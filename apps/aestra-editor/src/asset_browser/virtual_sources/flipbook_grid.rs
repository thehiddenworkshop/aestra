use super::*;
use crate::feathers::number_input::{
    DisplayNumberInput, NumberEdit, NumberInputValue, ScrubbableNumber, spawn_scalar_input,
};
use bevy::ecs::system::SystemParam;

const MAX_FRAMES: u32 = aestra_gpu::MAX_FLIPBOOK_FRAMES as u32;

#[derive(Component)]
pub(super) struct Host;

#[derive(Component)]
struct Draft {
    document: (aestra_core::EffectId, u64),
    texture: aestra_core::AssetId,
    image: Option<Handle<Image>>,
    columns: u32,
    rows: u32,
    frames: u32,
    fps: f32,
}

impl Draft {
    fn valid(&self) -> bool {
        self.frames <= self.columns * self.rows
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Columns,
    Rows,
    Frames,
    Fps,
}

#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    Confirm,
    Cancel,
}

#[derive(Component, Default)]
struct Preview {
    key: Option<(u32, u32, u32, Option<UVec2>)>,
}

#[derive(Component)]
struct GridMessage;

#[derive(SystemParam)]
pub(super) struct OpenGrid<'w, 's> {
    host: Query<'w, 's, Entity, With<Host>>,
    drafts: Query<'w, 's, (), With<Draft>>,
    catalog: Res<'w, ProjectEffectCatalog>,
    assets: Res<'w, AssetServer>,
    locale: Res<'w, Localizer>,
}

impl OpenGrid<'_, '_> {
    pub(super) fn open(
        &self,
        commands: &mut Commands,
        session: &EditorSession,
        texture: aestra_core::AssetId,
    ) {
        let Ok(host) = self.host.single() else { return };
        if !self.drafts.is_empty() {
            return;
        }
        let Some(asset) = session
            .effect
            .assets
            .iter()
            .find(|asset| asset.id == texture && asset.kind == AssetKind::Texture)
        else {
            return;
        };
        // Only registered project files may use an absolute AssetServer path.
        let image = self.catalog.root().canonicalize().ok().and_then(|root| {
            let path = root.join(&asset.path).canonicalize().ok()?;
            path.starts_with(&root).then(|| {
                self.assets
                    .load_builder()
                    .override_unapproved()
                    .load(bevy::asset::AssetPath::from_path_buf(path))
            })
        });
        let locale = &self.locale;
        commands.entity(host).with_children(|host| {
            host.spawn((
                Draft {
                    document: (session.effect.id, session.history_generation()),
                    texture,
                    image,
                    columns: 4,
                    rows: 1,
                    frames: 4,
                    fps: 12.0,
                },
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(6.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme::PANEL_DARK),
            ))
            .with_children(|draft| {
                draft
                    .spawn(Node {
                        column_gap: Val::Px(6.0),
                        row_gap: Val::Px(4.0),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|fields| {
                        for (field, label, value, max) in [
                            (
                                Field::Columns,
                                "browser-flipbook-columns",
                                4.0,
                                MAX_FRAMES as f32,
                            ),
                            (Field::Rows, "browser-flipbook-rows", 1.0, MAX_FRAMES as f32),
                            (
                                Field::Frames,
                                "browser-flipbook-frames",
                                4.0,
                                MAX_FRAMES as f32,
                            ),
                            (Field::Fps, "browser-flipbook-fps", 12.0, 120.0),
                        ] {
                            let label = locale.text(label);
                            fields
                                .spawn(Node {
                                    width: Val::Px(104.0),
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(4.0),
                                    ..default()
                                })
                                .with_children(|control| {
                                    chrome::caption(control, label.clone());
                                    control
                                        .spawn(Node {
                                            width: Val::Px(48.0),
                                            ..default()
                                        })
                                        .with_children(|number| {
                                            let input =
                                                spawn_scalar_input(number, value, &label, field);
                                            number.commands().entity(input).insert(
                                                ScrubbableNumber::new(value, 1.0, max, 1.0),
                                            );
                                        });
                                });
                        }
                    });
                draft.spawn((
                    Preview::default(),
                    Node {
                        column_gap: Val::Px(4.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ));
                draft
                    .spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|actions| {
                        spawn_feathers_action_button(
                            actions,
                            &locale.text("browser-flipbook-confirm"),
                            Action::Confirm,
                            false,
                        );
                        spawn_feathers_action_button(
                            actions,
                            &locale.text("browser-flipbook-cancel"),
                            Action::Cancel,
                            false,
                        );
                        let message = chrome::caption(actions, String::new());
                        actions.commands().entity(message).insert(GridMessage);
                    });
            });
        });
    }
}

pub(super) fn register(app: &mut App) {
    app.add_observer(edit).add_observer(activate).add_systems(
        Update,
        sync.after(DockingSet::Sync).before(AestraFeathersSet::Sync),
    );
}

fn edit(
    event: On<NumberEdit>,
    fields: Query<&Field>,
    mut drafts: Query<&mut Draft>,
    mut commands: Commands,
) {
    let (Ok(field), Ok(mut draft), NumberInputValue::F32(value)) =
        (fields.get(event.entity), drafts.single_mut(), event.value)
    else {
        return;
    };
    if !value.is_finite() {
        return;
    }
    let max = if matches!(field, Field::Fps) {
        120.0
    } else {
        MAX_FRAMES as f32
    };
    let value = value.round().clamp(1.0, max);
    match field {
        Field::Columns => draft.columns = value as u32,
        Field::Rows => draft.rows = value as u32,
        Field::Frames => draft.frames = value as u32,
        Field::Fps => draft.fps = value,
    }
    commands.trigger(DisplayNumberInput {
        entity: event.entity,
        value: NumberInputValue::F32(value),
    });
}

fn activate(
    event: On<Activate>,
    actions: Query<&Action>,
    drafts: Query<(Entity, &Draft)>,
    mut session: ResMut<EditorSession>,
    guard: crate::asset_drop::AuthoringDropGuard,
    mut commands: Commands,
) {
    let (Ok(action), Ok((entity, draft))) = (actions.get(event.entity), drafts.single()) else {
        return;
    };
    if matches!(action, Action::Cancel)
        || (guard.check().is_ok()
            && session.pending_change.is_none()
            && draft.document == (session.effect.id, session.history_generation())
            && draft.valid()
            && session.add_grid_flipbook_for_texture(
                draft.texture,
                draft.columns,
                draft.rows,
                draft.frames,
                draft.fps,
            ))
    {
        commands.entity(entity).despawn();
    }
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    session: Res<EditorSession>,
    browser: Res<AssetBrowserState>,
    drafts: Query<(Entity, &Draft)>,
    images: Option<Res<Assets<Image>>>,
    mut preview: Query<(Entity, &mut Preview)>,
    mut message: Query<&mut Text, With<GridMessage>>,
    actions: Query<(Entity, &Action)>,
    locale: Res<Localizer>,
) {
    let Ok((entity, draft)) = drafts.single() else {
        return;
    };
    if browser.scope != SourceScope::CurrentDocument
        || draft.document != (session.effect.id, session.history_generation())
        || !session
            .effect
            .assets
            .iter()
            .any(|asset| asset.id == draft.texture)
    {
        commands.entity(entity).despawn();
        return;
    }
    let size = draft
        .image
        .as_ref()
        .and_then(|image| images.as_ref()?.get(image))
        .map(Image::size);
    let key = (draft.columns, draft.rows, draft.frames, size);
    let Ok((entity, mut preview)) = preview.single_mut() else {
        return;
    };
    if preview.key == Some(key) && !locale.is_changed() {
        return;
    }
    preview.key = Some(key);
    if let Ok(mut message) = message.single_mut() {
        message.0 = locale.text(if draft.valid() {
            "browser-flipbook-order"
        } else {
            "browser-flipbook-grid-invalid"
        });
    }
    for (entity, action) in &actions {
        if matches!(action, Action::Confirm) {
            if draft.valid() {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
    }
    commands.entity(entity).despawn_children();
    commands.entity(entity).with_children(|strip| {
        for index in 0..draft.frames.min(draft.columns * draft.rows).min(8) {
            strip
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|frame| {
                    if let (Some(size), Some(image)) = (size, draft.image.as_ref()) {
                        let cell =
                            size.as_vec2() / Vec2::new(draft.columns as f32, draft.rows as f32);
                        let start = Vec2::new(
                            (index % draft.columns) as f32,
                            (index / draft.columns) as f32,
                        ) * cell;
                        let display = 32.0 * cell / cell.max_element();
                        frame.spawn((
                            ImageNode {
                                rect: Some(Rect::from_corners(start, start + cell)),
                                ..ImageNode::new(image.clone())
                            },
                            Node {
                                width: Val::Px(display.x),
                                height: Val::Px(display.y),
                                ..default()
                            },
                        ));
                    }
                    chrome::caption(frame, (index + 1).to_string());
                });
        }
        if draft.frames > 8 {
            chrome::caption(strip, "…".into());
        }
    });
}
