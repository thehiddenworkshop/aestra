//! Optional project-local editor metadata kept separate from semantic assets.

use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialProgramId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use tempfile::Builder as TempFileBuilder;

pub const PROJECT_EDITOR_LAYOUT_FORMAT_VERSION: u32 = 2;
pub const PROJECT_EDITOR_LAYOUT_DIRECTORY: &str = ".aestra";
pub const PROJECT_EDITOR_LAYOUT_FILE: &str = "editor-layout.ron";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialGraphViewportLayout {
    pub pan: [f32; 2],
    pub zoom: f32,
}

impl Default for MaterialGraphViewportLayout {
    fn default() -> Self {
        Self {
            pan: [0.0, 0.0],
            zoom: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialGraphNodeLayout {
    pub position: [f32; 2],
    pub collapsed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialGraphLayoutMetadata {
    pub viewport: Option<MaterialGraphViewportLayout>,
    pub nodes: BTreeMap<MaterialExpressionId, MaterialGraphNodeLayout>,
    pub output: Option<MaterialGraphNodeLayout>,
    pub visible_previews: BTreeSet<MaterialExpressionId>,
    pub output_preview_visible: bool,
}

impl MaterialGraphLayoutMetadata {
    pub fn retain_expressions(&mut self, expressions: &BTreeSet<MaterialExpressionId>) {
        self.nodes
            .retain(|expression, _| expressions.contains(expression));
        self.visible_previews
            .retain(|expression| expressions.contains(expression));
    }

    fn normalized(mut self) -> Self {
        if self.viewport.is_some_and(|viewport| {
            !viewport.pan.into_iter().all(f32::is_finite)
                || !viewport.zoom.is_finite()
                || viewport.zoom <= 0.0
        }) {
            self.viewport = None;
        }
        self.nodes
            .retain(|_, node| node.position.into_iter().all(f32::is_finite));
        if self
            .output
            .is_some_and(|node| !node.position.into_iter().all(f32::is_finite))
        {
            self.output = None;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectEditorLayout {
    pub format_version: u32,
    pub material_graphs: BTreeMap<MaterialProgramId, MaterialGraphLayoutMetadata>,
    /// Function-input nodes use expression IDs; the synthetic signature output uses `output`.
    /// Reuses the shared presentation schema; functions do not expose previews yet.
    pub function_graphs: BTreeMap<MaterialFunctionId, MaterialGraphLayoutMetadata>,
}

impl Default for ProjectEditorLayout {
    fn default() -> Self {
        Self {
            format_version: PROJECT_EDITOR_LAYOUT_FORMAT_VERSION,
            material_graphs: BTreeMap::new(),
            function_graphs: BTreeMap::new(),
        }
    }
}

impl ProjectEditorLayout {
    pub fn load(project_root: impl AsRef<Path>) -> io::Result<Self> {
        let path = project_editor_layout_path(project_root);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let layout: Self = ron::from_str(&source).map_err(io::Error::other)?;
        if layout.format_version > PROJECT_EDITOR_LAYOUT_FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "project editor layout format {} is newer than supported format {}",
                    layout.format_version, PROJECT_EDITOR_LAYOUT_FORMAT_VERSION
                ),
            ));
        }
        Ok(layout.normalized())
    }

    pub fn save(&self, project_root: impl AsRef<Path>) -> io::Result<()> {
        if self.format_version > PROJECT_EDITOR_LAYOUT_FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cannot save a newer editor layout format",
            ));
        }
        let path = project_editor_layout_path(project_root);
        let parent = path.parent().expect("editor layout path has a parent");
        fs::create_dir_all(parent)?;
        let source = ron::ser::to_string_pretty(
            &self.clone().normalized(),
            ron::ser::PrettyConfig::default(),
        )
        .map_err(io::Error::other)?;
        let mut temporary = TempFileBuilder::new()
            .prefix(".editor-layout-")
            .tempfile_in(parent)?;
        temporary.write_all(source.as_bytes())?;
        temporary.as_file().sync_all()?;
        let persisted = temporary.persist(&path).map_err(|error| error.error)?;
        persisted.sync_all()
    }

    fn normalized(mut self) -> Self {
        self.format_version = PROJECT_EDITOR_LAYOUT_FORMAT_VERSION;
        self.material_graphs = self
            .material_graphs
            .into_iter()
            .map(|(program, layout)| (program, layout.normalized()))
            .collect();
        self.function_graphs = self
            .function_graphs
            .into_iter()
            .map(|(function, layout)| (function, layout.normalized()))
            .collect();
        self
    }
}

pub fn project_editor_layout_path(project_root: impl AsRef<Path>) -> PathBuf {
    project_root
        .as_ref()
        .join(PROJECT_EDITOR_LAYOUT_DIRECTORY)
        .join(PROJECT_EDITOR_LAYOUT_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectAssetIndex;

    #[test]
    fn version_one_material_layout_migrates_and_coexists_with_function_layout() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgramId::from_u128(1);
        let function = MaterialFunctionId::from_u128(1);
        let expression = MaterialExpressionId::from_u128(2);
        let mut old = ProjectEditorLayout::default();
        old.material_graphs
            .entry(program)
            .or_default()
            .nodes
            .insert(
                expression,
                MaterialGraphNodeLayout {
                    position: [80.0, 35.0],
                    collapsed: true,
                },
            );
        let source = format!(
            "(format_version: 1, material_graphs: {})",
            ron::to_string(&old.material_graphs).unwrap()
        );
        let path = project_editor_layout_path(root.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source.as_bytes()).unwrap();
        let mut migrated = ProjectEditorLayout::load(root.path()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
        assert_eq!(migrated.material_graphs, old.material_graphs);
        assert!(migrated.function_graphs.is_empty());
        assert_eq!(
            migrated.format_version,
            PROJECT_EDITOR_LAYOUT_FORMAT_VERSION
        );
        let mut graph = migrated.material_graphs[&program].clone();
        graph.output = Some(MaterialGraphNodeLayout {
            position: [900.0, -25.0],
            collapsed: false,
        });
        graph.viewport = Some(MaterialGraphViewportLayout {
            pan: [24.0, -12.0],
            zoom: 1.25,
        });
        migrated.function_graphs.insert(function, graph);
        migrated.save(root.path()).unwrap();
        assert_eq!(ProjectEditorLayout::load(root.path()).unwrap(), migrated);
    }

    #[test]
    fn newer_in_memory_layout_cannot_be_downgraded_by_save() {
        let root = tempfile::tempdir().unwrap();
        let mut layout = ProjectEditorLayout::default();
        layout.save(root.path()).unwrap();
        let path = project_editor_layout_path(root.path());
        let before = fs::read(&path).unwrap();
        layout.format_version += 1;
        assert!(layout.save(root.path()).is_err());
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    fn project_editor_layout_round_trips_stable_material_ids() {
        let temporary = tempfile::tempdir().unwrap();
        let program = MaterialProgramId::from_u128(0xA001);
        let expression = MaterialExpressionId::from_u128(0xE001);
        let mut layout = ProjectEditorLayout::default();
        layout.material_graphs.insert(
            program,
            MaterialGraphLayoutMetadata {
                viewport: Some(MaterialGraphViewportLayout {
                    pan: [42.0, -18.0],
                    zoom: 1.25,
                }),
                nodes: BTreeMap::from([(
                    expression,
                    MaterialGraphNodeLayout {
                        position: [320.0, 180.0],
                        collapsed: true,
                    },
                )]),
                output: Some(MaterialGraphNodeLayout {
                    position: [640.0, 220.0],
                    collapsed: false,
                }),
                visible_previews: BTreeSet::from([expression]),
                output_preview_visible: true,
            },
        );

        layout.save(temporary.path()).unwrap();

        assert_eq!(ProjectEditorLayout::load(temporary.path()).unwrap(), layout);
        assert!(project_editor_layout_path(temporary.path()).is_file());
        assert!(
            ProjectAssetIndex::scan(temporary.path())
                .diagnostics()
                .is_empty()
        );

        layout.material_graphs.get_mut(&program).unwrap().viewport =
            Some(MaterialGraphViewportLayout {
                pan: [-5.0, 9.0],
                zoom: 0.75,
            });
        layout.save(temporary.path()).unwrap();
        assert_eq!(ProjectEditorLayout::load(temporary.path()).unwrap(), layout);
    }

    #[test]
    fn missing_layout_is_optional() {
        let temporary = tempfile::tempdir().unwrap();

        assert_eq!(
            ProjectEditorLayout::load(temporary.path()).unwrap(),
            ProjectEditorLayout::default()
        );
    }

    #[test]
    fn identical_material_ids_have_independent_project_layout_files() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let program = MaterialProgramId::from_u128(0xA001);
        let expression = MaterialExpressionId::from_u128(0xE001);
        let mut first = ProjectEditorLayout::default();
        first
            .material_graphs
            .entry(program)
            .or_default()
            .nodes
            .insert(
                expression,
                MaterialGraphNodeLayout {
                    position: [80.0, -40.0],
                    collapsed: true,
                },
            );
        let mut second = first.clone();
        second
            .material_graphs
            .get_mut(&program)
            .unwrap()
            .nodes
            .get_mut(&expression)
            .unwrap()
            .position = [900.0, 150.0];

        first.save(first_root.path()).unwrap();
        second.save(second_root.path()).unwrap();
        assert_eq!(ProjectEditorLayout::load(first_root.path()).unwrap(), first);
        assert_eq!(
            ProjectEditorLayout::load(second_root.path()).unwrap(),
            second
        );
    }

    #[test]
    fn legacy_metadata_without_optional_node_fields_loads_with_defaults() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgramId::from_u128(0xA001);
        let expression = MaterialExpressionId::from_u128(0xE001);
        let path = project_editor_layout_path(root.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let program_key = ron::to_string(&program).unwrap();
        let expression_key = ron::to_string(&expression).unwrap();
        fs::write(&path, format!(
            "(material_graphs: {{{program_key}: (nodes: {{{expression_key}: (position: (32.0, 64.0))}})}})"
        )).unwrap();

        let restored = ProjectEditorLayout::load(root.path()).unwrap();
        let graph = &restored.material_graphs[&program];
        assert_eq!(graph.nodes[&expression].position, [32.0, 64.0]);
        assert!(!graph.nodes[&expression].collapsed);
        assert!(graph.viewport.is_none());
        assert!(graph.output.is_none());
        assert!(graph.visible_previews.is_empty());
        assert!(!graph.output_preview_visible);
    }

    #[test]
    fn future_layout_versions_are_rejected_without_affecting_project_assets() {
        let temporary = tempfile::tempdir().unwrap();
        let path = project_editor_layout_path(temporary.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            format!(
                "(format_version: {}, material_graphs: {{}})",
                PROJECT_EDITOR_LAYOUT_FORMAT_VERSION + 1
            ),
        )
        .unwrap();

        let original = fs::read(&path).unwrap();
        let error = ProjectEditorLayout::load(temporary.path()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(
            ProjectAssetIndex::scan(temporary.path())
                .diagnostics()
                .is_empty()
        );
    }

    #[test]
    fn stale_expression_layout_is_pruned_without_touching_other_programs() {
        let retained = MaterialExpressionId::from_u128(0xE001);
        let stale = MaterialExpressionId::from_u128(0xE002);
        let mut graph = MaterialGraphLayoutMetadata {
            nodes: BTreeMap::from([
                (retained, MaterialGraphNodeLayout::default()),
                (stale, MaterialGraphNodeLayout::default()),
            ]),
            visible_previews: BTreeSet::from([retained, stale]),
            ..Default::default()
        };

        graph.retain_expressions(&BTreeSet::from([retained]));

        assert_eq!(graph.nodes.keys().copied().collect::<Vec<_>>(), [retained]);
        assert_eq!(
            graph.visible_previews.iter().copied().collect::<Vec<_>>(),
            [retained]
        );
    }

    #[test]
    fn invalid_geometry_is_discarded_on_load() {
        let temporary = tempfile::tempdir().unwrap();
        let program = MaterialProgramId::from_u128(0xA001);
        let expression = MaterialExpressionId::from_u128(0xE001);
        let mut layout = ProjectEditorLayout::default();
        layout.material_graphs.insert(
            program,
            MaterialGraphLayoutMetadata {
                viewport: Some(MaterialGraphViewportLayout {
                    pan: [f32::NAN, 0.0],
                    zoom: 1.0,
                }),
                nodes: BTreeMap::from([(
                    expression,
                    MaterialGraphNodeLayout {
                        position: [f32::INFINITY, 0.0],
                        collapsed: false,
                    },
                )]),
                ..Default::default()
            },
        );
        layout.save(temporary.path()).unwrap();

        let restored = ProjectEditorLayout::load(temporary.path()).unwrap();
        let restored = &restored.material_graphs[&program];
        assert!(restored.viewport.is_none());
        assert!(restored.nodes.is_empty());
    }
}
