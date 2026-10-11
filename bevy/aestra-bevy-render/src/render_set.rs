/// Scheduling point applications use to update [`crate::PresentedEffect`] before rendering consumes it.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AestraRenderSet {
    Prepare,
}
