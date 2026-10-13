//! Explicit Delete/Restore coordinator and version-shared dialog composition.
mod documents;
mod history;
mod panel;
pub(super) mod reference_checks;

pub(super) use panel::{Open, register};
pub(crate) use panel::{history_step, spawn};

#[cfg(test)]
use crate::{project_content::io, *};
#[cfg(test)]
use aestra_project::ProjectSourceId;
#[cfg(test)]
use bevy::{input_focus::InputFocus, ui_widgets::Activate};
#[cfg(test)]
use panel::{Body, Choice, State, show_error};
#[cfg(test)]
mod tests;
