//! Semantic history context selection on the actual editor session.
use crate::history::asset_order::Context;
use crate::{material_document::MaterialEditingTarget, session::EditorSession};

impl Context {
    pub(crate) fn current(session: &EditorSession) -> Self {
        if (session.standalone_function().is_some() || session.standalone_material().is_some())
            && session.material_history_active
        {
            Self::Material(session.material_target.clone())
        } else {
            Self::Effect
        }
    }
    pub(crate) fn select(&self, session: &mut EditorSession) {
        match self {
            Self::Effect => {
                session.material_target = MaterialEditingTarget::EffectInstance;
                session.material_history_active = false;
            }
            Self::Material(target) => {
                session.material_target = target.clone();
                session.material_history_active = true;
            }
        }
    }
}
