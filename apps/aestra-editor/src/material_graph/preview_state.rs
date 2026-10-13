//! Actual preview visibility and raster-cache data, independent of raster worker scheduling.
use super::MaterialGraphPreviewTarget;
use aestra_core::{
    MaterialExpressionId, MaterialId, MaterialProgramId,
    material::{MaterialInstance, MaterialProgram, MaterialValueType},
};
use bevy::prelude::{Handle, Image, Resource};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialGraphPreviewState {
    pub(super) visible: BTreeSet<(MaterialProgramId, MaterialGraphPreviewTarget)>,
    pub(super) cache: BTreeMap<Key, MaterialGraphPreviewCache>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key {
    pub program: MaterialProgramId,
    pub target: MaterialGraphPreviewTarget,
    pub(super) root: PathBuf,
    pub(super) instance: Option<MaterialId>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Input {
    pub(super) program: MaterialProgram,
    pub(super) instance: Option<MaterialInstance>,
    pub(super) functions: Vec<aestra_core::material::MaterialFunction>,
    pub(super) value_type: Option<MaterialValueType>,
}

#[derive(Debug, Clone)]
pub(super) struct MaterialGraphPreviewCache {
    pub(super) input: Input,
    pub(super) image: Handle<Image>,
}

impl MaterialGraphPreviewState {
    pub(crate) fn is_visible(
        &self,
        program: MaterialProgramId,
        target: MaterialGraphPreviewTarget,
    ) -> bool {
        self.visible.contains(&(program, target))
    }

    pub(super) fn toggle(
        &mut self,
        program: MaterialProgramId,
        target: MaterialGraphPreviewTarget,
    ) -> bool {
        let key = (program, target);
        if self.visible.remove(&key) {
            false
        } else {
            self.visible.insert(key);
            true
        }
    }

    pub(crate) fn set_visible(
        &mut self,
        program: MaterialProgramId,
        targets: impl IntoIterator<Item = MaterialGraphPreviewTarget>,
        visible: bool,
    ) {
        for target in targets {
            let key = (program, target);
            if visible {
                self.visible.insert(key);
            } else {
                self.visible.remove(&key);
            }
        }
    }

    pub(super) fn visible_expressions(
        &self,
        program: MaterialProgramId,
    ) -> BTreeSet<MaterialExpressionId> {
        self.visible
            .iter()
            .filter_map(|(candidate, target)| {
                (*candidate == program)
                    .then_some(*target)
                    .and_then(|target| match target {
                        MaterialGraphPreviewTarget::Expression(expression) => Some(expression),
                        MaterialGraphPreviewTarget::Output => None,
                    })
            })
            .collect()
    }

    pub(super) fn output_visible(&self, program: MaterialProgramId) -> bool {
        self.is_visible(program, MaterialGraphPreviewTarget::Output)
    }

    pub(super) fn restore_layout(
        &mut self,
        program: MaterialProgramId,
        expressions: &BTreeSet<MaterialExpressionId>,
        output_visible: bool,
    ) {
        self.visible.retain(|(candidate, _)| *candidate != program);
        self.visible.extend(
            expressions
                .iter()
                .copied()
                .map(|expression| (program, MaterialGraphPreviewTarget::Expression(expression))),
        );
        if output_visible {
            self.visible
                .insert((program, MaterialGraphPreviewTarget::Output));
        }
    }
}
