//! Actual function-open document intent shared by browser and graph consumers.
use bevy::prelude::Event;
#[derive(Event)]
pub(crate) struct OpenFunction {
    pub(crate) function: aestra_core::MaterialFunctionId,
    pub(crate) new_view: bool,
}
