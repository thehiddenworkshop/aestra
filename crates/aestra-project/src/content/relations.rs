//! Direct authored references captured during discovery, not disk-validating mutation preflight.
use super::*;
use crate::ProjectAssetIndexAvailability;
use aestra_core::{
    AssetId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunctionRef,
        MaterialPresetGraphNodeKind, MaterialPresetRecipe, MaterialProgramRef, MaterialValue,
    },
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectRelationTarget {
    Asset(ProjectAssetId),
    /// Built-ins have semantic identity but must never resolve to project files with the same ID.
    BuiltIn(ProjectAssetId),
    /// Effect-local resource declarations refer to root-relative files, not project asset IDs.
    File(std::path::PathBuf),
    /// A material resource binding needs an effect-instance context to locate its file.
    ContextualResource(AssetId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSourceRelation {
    pub owner: ProjectSourceId,
    pub target: ProjectRelationTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRelationStatus {
    Available,
    Missing,
    Ambiguous,
    Unavailable,
    BuiltIn,
    ContextRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSourceRelations {
    pub dependencies: Vec<ProjectSourceRelation>,
    pub usages: Vec<ProjectSourceRelation>,
    /// False for unreadable/unparsed sources and file formats whose references are not indexed.
    pub dependencies_known: bool,
    /// Completeness is scoped to indexed authored references, not arbitrary scripts/shaders.
    pub usages_complete: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RelationIndex {
    outgoing: BTreeMap<ProjectSourceId, Vec<ProjectSourceRelation>>,
    incoming: BTreeMap<ProjectRelationTarget, Vec<ProjectSourceRelation>>,
}

impl RelationIndex {
    pub(super) fn build(documents: &BTreeMap<ProjectSourceId, ProjectSourceDocument>) -> Self {
        let mut index = Self::default();
        for (&owner, document) in documents {
            let mut targets = BTreeSet::new();
            match document {
                ProjectSourceDocument::Effect(effect) => {
                    for clip in &effect.effect_clips {
                        targets.insert(ProjectRelationTarget::Asset(clip.source.into()));
                    }
                    for instance in &effect.material_instances {
                        targets.insert(match instance.program {
                            MaterialProgramRef::Project(id) => {
                                ProjectRelationTarget::Asset(ProjectAssetId::MaterialProgram(id))
                            }
                            MaterialProgramRef::BuiltIn(id) => {
                                ProjectRelationTarget::BuiltIn(ProjectAssetId::MaterialProgram(id))
                            }
                        });
                    }
                    for asset in &effect.assets {
                        targets.insert(ProjectRelationTarget::File(std::path::PathBuf::from(
                            asset.path.replace('\\', "/"),
                        )));
                    }
                }
                ProjectSourceDocument::MaterialProgram(program) => {
                    expression_targets(&program.expressions, &mut targets);
                    for parameter in &program.parameters {
                        if let Some(MaterialValue::Texture2D(id)) = parameter.default {
                            targets.insert(ProjectRelationTarget::ContextualResource(id));
                        }
                    }
                }
                ProjectSourceDocument::MaterialFunction(function) => {
                    expression_targets(&function.expressions, &mut targets);
                    for input in &function.inputs {
                        if let Some(MaterialValue::Texture2D(id)) = input.default {
                            targets.insert(ProjectRelationTarget::ContextualResource(id));
                        }
                    }
                }
                // Portable recipes have no retained links to installation sites. Texture values
                // still require the destination effect's resource table, just like program defaults.
                ProjectSourceDocument::MaterialPreset(preset) => match &preset.recipe {
                    MaterialPresetRecipe::Stack { defaults, .. } => {
                        for value in defaults {
                            if let MaterialValue::Texture2D(id) = value.value {
                                targets.insert(ProjectRelationTarget::ContextualResource(id));
                            }
                        }
                    }
                    MaterialPresetRecipe::Graph(recipe) => {
                        for node in &recipe.nodes {
                            if let MaterialPresetGraphNodeKind::Constant(
                                MaterialValue::Texture2D(id),
                            ) = node.kind
                            {
                                targets.insert(ProjectRelationTarget::ContextualResource(id));
                            }
                        }
                    }
                },
            }
            let edges = targets
                .into_iter()
                .map(|target| ProjectSourceRelation { owner, target })
                .collect::<Vec<_>>();
            for edge in &edges {
                index
                    .incoming
                    .entry(edge.target.clone())
                    .or_default()
                    .push(edge.clone());
            }
            index.outgoing.insert(owner, edges);
        }
        index
    }
}

fn expression_targets(
    expressions: &[MaterialExpression],
    targets: &mut BTreeSet<ProjectRelationTarget>,
) {
    for expression in expressions {
        let target = match expression.kind {
            MaterialExpressionKind::CustomWeslCall { function, .. } => {
                ProjectRelationTarget::Asset(ProjectAssetId::MaterialFunction(function))
            }
            MaterialExpressionKind::FunctionCall { function, .. } => match function {
                MaterialFunctionRef::Project(id) => {
                    ProjectRelationTarget::Asset(ProjectAssetId::MaterialFunction(id))
                }
                MaterialFunctionRef::BuiltIn(id) => {
                    ProjectRelationTarget::BuiltIn(ProjectAssetId::MaterialFunction(id))
                }
            },
            MaterialExpressionKind::Constant(MaterialValue::Texture2D(id)) => {
                ProjectRelationTarget::ContextualResource(id)
            }
            _ => continue,
        };
        targets.insert(target);
    }
}

impl ProjectContent {
    /// Snapshot-only, direct relations. Duplicate sources remain independently inspectable.
    /// Do not use this report to authorize destructive operations.
    pub fn source_relations(&self, source: ProjectSourceId) -> ProjectSourceRelations {
        let dependencies = self
            .relations
            .outgoing
            .get(&source)
            .cloned()
            .unwrap_or_default();
        let mut usages = Vec::new();
        if let Some(asset) = self.asset_for_source(source) {
            usages.extend(
                self.relations
                    .incoming
                    .get(&ProjectRelationTarget::Asset(asset))
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
        }
        // File references use the same lexical, root-relative key on every platform.
        if let Some(entry) = self.source(source) {
            let path = entry.relative_path.to_string_lossy().replace('\\', "/");
            usages.extend(
                self.relations
                    .incoming
                    .get(&ProjectRelationTarget::File(path.into()))
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
        }
        usages.sort_by_key(|edge| {
            self.source(edge.owner)
                .map(|source| source.relative_path.clone())
        });
        ProjectSourceRelations {
            dependencies,
            usages,
            dependencies_known: self.relations.outgoing.contains_key(&source),
            usages_complete: self.asset_index.diagnostics().is_empty()
                && self.source_tree.diagnostics().is_empty(),
        }
    }

    /// All candidates, never an arbitrary winner for duplicated semantic identity.
    pub fn relation_sources(&self, target: &ProjectRelationTarget) -> Vec<ProjectSourceId> {
        match target {
            ProjectRelationTarget::Asset(asset) => self.sources_for_asset(*asset).to_vec(),
            ProjectRelationTarget::File(path) => self
                .source_tree
                .at_relative_path(path)
                .map(|entry| vec![entry.id])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub fn relation_status(&self, target: &ProjectRelationTarget) -> ProjectRelationStatus {
        match target {
            ProjectRelationTarget::BuiltIn(_) => return ProjectRelationStatus::BuiltIn,
            ProjectRelationTarget::ContextualResource(_) => {
                return ProjectRelationStatus::ContextRequired;
            }
            _ => {}
        }
        if !matches!(
            self.source_tree.availability(),
            ProjectAssetIndexAvailability::Ready
        ) {
            return ProjectRelationStatus::Unavailable;
        }
        let sources = self.relation_sources(target);
        match sources.as_slice() {
            [] => ProjectRelationStatus::Missing,
            [id] => {
                let entry = self
                    .source(*id)
                    .expect("relation candidate is in the snapshot");
                let invalid = entry.error.is_some()
                    || !matches!(entry.kind, ProjectSourceKind::File(_))
                    || self
                        .asset_index
                        .diagnostics()
                        .iter()
                        .any(|diagnostic| diagnostic.path.as_ref() == Some(&entry.path));
                if invalid {
                    ProjectRelationStatus::Unavailable
                } else {
                    ProjectRelationStatus::Available
                }
            }
            _ => ProjectRelationStatus::Ambiguous,
        }
    }
}
