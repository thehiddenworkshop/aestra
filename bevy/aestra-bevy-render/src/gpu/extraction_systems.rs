//! Custom main-to-render extraction, using the selected engine adapter's parameter.
use super::{
    catchup_pacing::{AestraCatchupPacing, CatchupPacer},
    extraction::Extract,
};
use bevy::prelude::*;

/// Absence restores the default; mutating only this switch retains adaptive history.
pub(super) fn extract_catchup_pacing(
    pacing: Extract<Option<Res<AestraCatchupPacing>>>,
    mut pacer: ResMut<CatchupPacer>,
) {
    pacer.set_paced(pacing.as_ref().is_none_or(|pacing| pacing.paced));
}
