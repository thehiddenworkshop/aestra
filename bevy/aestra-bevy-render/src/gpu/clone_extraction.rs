//! Version-neutral implementations for clone-only extraction payloads.
//! Trait paths are supplied explicitly by each engine adapter.
macro_rules! clone_component {
    ($component:ty, $sync:path, $extract:path) => {
        impl $sync for $component {
            type Target = Self;
        }
        impl $extract for $component {
            type QueryData = &'static Self;
            type QueryFilter = ();
            type Out = Self;
            fn extract_component(
                source: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>,
            ) -> Option<Self::Out> {
                Some(source.clone())
            }
        }
    };
}
macro_rules! clone_resource {
    ($resource:ty, $extract:path) => {
        impl $extract for $resource {
            type Source = Self;
            fn extract_resource(source: &Self) -> Self {
                source.clone()
            }
        }
    };
}
pub(super) use {clone_component, clone_resource};
