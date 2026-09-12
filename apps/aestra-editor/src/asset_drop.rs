//! Shared asset-drop transport and feedback. Target adapters own validation and edits.
//! Filesystem operations deliberately retain their serialized I/O and recovery path.
mod payload;
pub(crate) use payload::{AssetPayload, AuthoringDropGuard};

use crate::*;
use bevy::picking::{events::DragLeave, pointer::PointerButton};

/// Resolve the nearest eligible ancestor, so nested input fields win over their cards.
pub(crate) fn nearest<T>(
    entity: Entity,
    parents: &Query<&ChildOf>,
    mut resolve: impl FnMut(Entity) -> Option<T>,
) -> Option<(Entity, T)> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find_map(|entity| resolve(entity).map(|target| (entity, target)))
}

/// The same source/target resolution is used on hover and release. Never fall back to
/// a parent after the nearest target rejects a type: that would trigger another action.
pub(crate) fn resolve<T>(
    button: PointerButton,
    source: Entity,
    destination: Entity,
    sources: &Query<&AssetPayload>,
    parents: &Query<&ChildOf>,
    target: impl FnMut(Entity) -> Option<T>,
) -> Option<(AssetPayload, Entity, T)> {
    if button != PointerButton::Primary {
        return None;
    }
    let (_, payload) = nearest(source, parents, |entity| sources.get(entity).ok().cloned())?;
    let (entity, target) = nearest(destination, parents, target)?;
    Some((payload, entity, target))
}

pub(crate) fn cancelled(keys: Option<&ButtonInput<KeyCode>>) -> bool {
    keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
}

pub(crate) fn active(
    payload: &AssetPayload,
    sources: &Query<&AssetPayload>,
    catalog: Option<&ProjectEffectCatalog>,
    keys: Option<&ButtonInput<KeyCode>>,
) -> bool {
    !cancelled(keys)
        && catalog.is_some_and(|catalog| payload.resolve(catalog).is_ok())
        && sources.iter().any(|source| source == payload)
}

/// Owner-scoped feedback prevents one consumer's cleanup from clearing another's preview.
#[derive(Component)]
pub(crate) struct Feedback<T: Send + Sync + 'static> {
    pub payload: AssetPayload,
    pub target: Entity,
    marker: std::marker::PhantomData<fn() -> T>,
}

struct FeedbackPlugin<T: Send + Sync + 'static>(std::marker::PhantomData<fn() -> T>);
impl<T: Send + Sync + 'static> Plugin for FeedbackPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_observer(leave::<T>)
            .add_systems(Update, cleanup::<T>);
    }
}

pub(crate) fn register_feedback<T: Send + Sync + 'static>(app: &mut App) {
    if !app.is_plugin_added::<FeedbackPlugin<T>>() {
        app.add_plugins(FeedbackPlugin::<T>(std::marker::PhantomData));
    }
}

pub(crate) fn clear_feedback<T: Send + Sync + 'static>(
    feedback: &Query<Entity, With<Feedback<T>>>,
    commands: &mut Commands,
) {
    for entity in feedback {
        commands.entity(entity).try_despawn();
    }
}

pub(crate) fn show_feedback<T: Send + Sync + 'static>(
    commands: &mut Commands,
    entity: Entity,
    payload: AssetPayload,
    label: String,
    accepted: bool,
) {
    let color = if accepted {
        theme::ACCENT
    } else {
        Color::srgb(0.95, 0.3, 0.3)
    };
    commands.entity(entity).with_children(|parent| {
        parent
            .spawn((
                Feedback::<T> {
                    payload,
                    target: entity,
                    marker: std::marker::PhantomData,
                },
                Pickable::IGNORE,
                GlobalZIndex(275),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(color),
            ))
            .with_child((
                Text::new(label),
                TextColor(theme::TEXT),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                Pickable::IGNORE,
                BackgroundColor(theme::PANEL),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    right: Val::Px(0.0),
                    max_width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(5.0)),
                    ..default()
                },
            ));
    });
}

fn leave<T: Send + Sync + 'static>(
    event: On<Pointer<DragLeave>>,
    parents: Query<&ChildOf>,
    feedback: Query<(Entity, &Feedback<T>)>,
    mut commands: Commands,
) {
    for (entity, marker) in &feedback {
        if nearest(event.entity, &parents, |candidate| {
            (candidate == marker.target).then_some(())
        })
        .is_some()
        {
            commands.entity(entity).try_despawn();
        }
    }
}

fn cleanup<T: Send + Sync + 'static>(
    sources: Query<&AssetPayload>,
    feedback: Query<(Entity, &Feedback<T>)>,
    entities: Query<()>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    for (entity, marker) in &feedback {
        if !entities.contains(marker.target)
            || !active(
                &marker.payload,
                &sources,
                catalog.as_deref(),
                keys.as_deref(),
            )
        {
            commands.entity(entity).try_despawn();
        }
    }
}

#[cfg(test)]
mod tests;
