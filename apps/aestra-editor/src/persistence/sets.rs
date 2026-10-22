//! Production document persistence scheduling identities.
use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PersistenceSet {
    Startup,
    Actions,
    Lifecycle,
}
