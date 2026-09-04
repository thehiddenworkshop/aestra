//! Optional project-local editor metadata kept separate from semantic assets.

use aestra_core::{MaterialExpressionId, MaterialProgramId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use tempfile::Builder as TempFileBuilder;

pub const PROJECT_EDITOR_LAYOUT_FORMAT_VERSION: u32 = 1;
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
}

impl Default for ProjectEditorLayout {
    fn default() -> Self {
        Self {
            format_version: PROJECT_EDITOR_LAYOUT_FORMAT_VERSION,
            material_graphs: BTreeMap::new(),
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

        let error = ProjectEditorLayout::load(temporary.path()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
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
