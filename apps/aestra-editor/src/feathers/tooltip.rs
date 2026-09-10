//! Delayed, window-aware editor tooltips.

use crate::theme;
use bevy::{
    picking::events::{DragStart, Over, Pointer},
    prelude::*,
    ui::RelativeCursorPosition,
    ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    window::PrimaryWindow,
};
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_TOOLTIP_DELAY: Duration = Duration::from_millis(650);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EditorTooltipSide {
    #[default]
    Left,
    Right,
}

/// How a tooltip is positioned relative to its hovered target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EditorTooltipAnchor {
    /// Anchored to the hovered element's bounds. Correct for ordinary chrome laid out in place.
    #[default]
    Element,
    /// Anchored beside the cursor, outside the target's surface. Use for targets that live under a
    /// transformed/clipped surface (e.g. the pan/zoom graph canvas), where the element bounds do
    /// not match the visible position and clipping would cut the popup.
    Cursor,
}

/// Resolved editor-facing tooltip content.
///
/// Strings are localized by the call site before this component is inserted. Keeping localization
/// outside the widget lets the same tooltip render asset names, shortcuts, and generated text too.
#[derive(Component, Clone, Debug)]
#[require(RelativeCursorPosition)]
pub(crate) struct EditorTooltip {
    title: Option<String>,
    description: String,
    shortcut: Option<String>,
    footer: Option<String>,
    delay: Duration,
    preferred_side: EditorTooltipSide,
    anchor: EditorTooltipAnchor,
}

impl EditorTooltip {
    pub(crate) fn description(description: impl Into<String>) -> Self {
        Self {
            title: None,
            description: description.into(),
            shortcut: None,
            footer: None,
            delay: DEFAULT_TOOLTIP_DELAY,
            preferred_side: EditorTooltipSide::Left,
            anchor: EditorTooltipAnchor::Element,
        }
    }

    pub(crate) fn titled(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::description(description)
        }
    }

    pub(crate) fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    #[allow(dead_code)] // Part of the shared tooltip contract; no current surface needs a footer.
    pub(crate) fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    #[allow(dead_code)] // The default is currently consistent across every editor surface.
    pub(crate) fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub(crate) fn with_preferred_side(mut self, side: EditorTooltipSide) -> Self {
        self.preferred_side = side;
        self
    }

    /// Anchors the tooltip beside the cursor instead of the element. Required for targets under the
    /// pan/zoom graph canvas, whose element bounds and clip do not match the visible position.
    pub(crate) fn anchored_to_cursor(mut self) -> Self {
        self.anchor = EditorTooltipAnchor::Cursor;
        self
    }

    pub(crate) fn accessible_label(&self) -> String {
        [
            self.title.as_deref(),
            Some(self.description.as_str()),
            self.shortcut.as_deref(),
            self.footer.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(". ")
    }
}

#[derive(Component)]
pub(crate) struct TooltipPopup;

#[derive(Resource, Default)]
pub(crate) struct TooltipState {
    target: Option<Entity>,
    hovered_at: Option<Instant>,
    popup: Option<Entity>,
}

pub(crate) fn begin_tooltip(
    over: On<Pointer<Over>>,
    tooltips: Query<(), With<EditorTooltip>>,
    mut state: ResMut<TooltipState>,
    mut commands: Commands,
) {
    if !tooltips.contains(over.entity) || state.target == Some(over.entity) {
        return;
    }
    clear_popup(&mut commands, &mut state);
    state.target = Some(over.entity);
    state.hovered_at = Some(Instant::now());
}

/// Dragging starts an edit or layout operation, so stale hover help must not obscure it.
pub(crate) fn dismiss_tooltip_on_drag(
    _drag: On<Pointer<DragStart>>,
    mut state: ResMut<TooltipState>,
    mut commands: Commands,
) {
    clear_tooltip(&mut commands, &mut state);
}

pub(crate) fn update_tooltip(
    mut commands: Commands,
    mut state: ResMut<TooltipState>,
    tooltips: Query<(&EditorTooltip, &RelativeCursorPosition)>,
    popups: Query<(), With<TooltipPopup>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    parents: Query<&ChildOf>,
    cameras: Query<&UiTargetCamera>,
) {
    if state.popup.is_some_and(|popup| !popups.contains(popup)) {
        state.popup = None;
    }
    let Some(target) = state.target else {
        return;
    };
    let Ok((tooltip, cursor)) = tooltips.get(target) else {
        clear_tooltip(&mut commands, &mut state);
        return;
    };
    if !cursor.cursor_over() {
        clear_tooltip(&mut commands, &mut state);
        return;
    }
    if state.popup.is_some()
        || state
            .hovered_at
            .is_none_or(|started| started.elapsed() < tooltip.delay)
    {
        return;
    }

    let content = tooltip.clone();
    let accessible_label = content.accessible_label();
    let positions = tooltip_positions(content.preferred_side);
    match content.anchor {
        EditorTooltipAnchor::Element => {
            let mut popup = None;
            commands.entity(target).with_children(|parent| {
                popup = Some(spawn_tooltip_body(
                    parent,
                    &content,
                    positions,
                    accessible_label,
                    true,
                ));
            });
            state.popup = popup;
        }
        EditorTooltipAnchor::Cursor => {
            // The target lives under the transformed, clipped graph canvas, so anchor a fresh
            // root node beside the cursor (in window space) and hang the popup off that instead.
            let Ok(window) = windows.single() else {
                return;
            };
            let Some(cursor) = window.cursor_position() else {
                return;
            };
            // The root anchor no longer inherits the target's camera, so resolve the nearest one
            // up the tree and pin it explicitly (falling back to the default UI camera).
            let camera = std::iter::once(target)
                .chain(parents.iter_ancestors(target))
                .find_map(|entity| cameras.get(entity).ok().cloned());
            let mut anchor = commands.spawn((
                TooltipPopup,
                Pickable::IGNORE,
                GlobalZIndex(300),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(cursor.x),
                    top: Val::Px(cursor.y),
                    ..default()
                },
            ));
            if let Some(camera) = camera {
                anchor.insert(camera);
            }
            let anchor = anchor.id();
            commands.entity(anchor).with_children(|parent| {
                spawn_tooltip_body(parent, &content, positions, accessible_label, false);
            });
            state.popup = Some(anchor);
        }
    }
}

/// Spawns the tooltip popup (a window-aware [`Popover`] with the resolved content). `tracked` marks
/// it with [`TooltipPopup`] so the lifecycle can find it; the cursor anchor carries that marker
/// instead, so its inner body passes `false`.
fn spawn_tooltip_body(
    parent: &mut ChildSpawnerCommands,
    content: &EditorTooltip,
    positions: Vec<PopoverPlacement>,
    accessible_label: String,
    tracked: bool,
) -> Entity {
    let mut body = parent.spawn((
        Popover {
            positions,
            window_margin: 10.0,
        },
        OverrideClip,
        GlobalZIndex(300),
        Pickable::IGNORE,
        AccessibleLabel(accessible_label),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(280.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL),
        BorderColor::all(theme::BORDER_BRIGHT),
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.65),
            Val::Px(0.0),
            Val::Px(2.0),
            Val::Px(3.0),
            Val::Px(5.0),
        ),
    ));
    if tracked {
        body.insert(TooltipPopup);
    }
    body.with_children(|popup| {
        // Render the title as a direct child (like the description) so it always lays out; only a
        // title paired with a shortcut needs the space-between header row.
        match (content.title.as_deref(), content.shortcut.as_deref()) {
            (Some(title), Some(shortcut)) => {
                popup
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(14.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: Val::Px(10.0),
                        ..default()
                    })
                    .with_children(|header| {
                        header.spawn(tooltip_text(title, 11.0, theme::TEXT));
                        header.spawn(tooltip_text(shortcut, 9.0, theme::TEXT_MUTED));
                    });
            }
            (Some(title), None) => {
                popup.spawn(tooltip_text(title, 11.0, theme::TEXT));
            }
            (None, Some(shortcut)) => {
                popup.spawn(tooltip_text(shortcut, 9.0, theme::TEXT_MUTED));
            }
            (None, None) => {}
        }
        popup.spawn(tooltip_text(&content.description, 10.0, theme::TEXT));
        if let Some(footer) = content.footer.as_ref() {
            popup
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::top(Val::Px(5.0)),
                        border: UiRect::top(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(theme::BORDER),
                ))
                .with_child(tooltip_text(footer, 9.0, theme::TEXT_MUTED));
        }
    });
    body.id()
}

fn tooltip_positions(preferred_side: EditorTooltipSide) -> Vec<PopoverPlacement> {
    let (preferred, opposite) = match preferred_side {
        EditorTooltipSide::Left => (PopoverSide::Left, PopoverSide::Right),
        EditorTooltipSide::Right => (PopoverSide::Right, PopoverSide::Left),
    };
    vec![
        PopoverPlacement {
            side: preferred,
            align: PopoverAlign::Center,
            gap: 10.0,
        },
        PopoverPlacement {
            side: opposite,
            align: PopoverAlign::Center,
            gap: 10.0,
        },
        PopoverPlacement {
            side: PopoverSide::Bottom,
            align: PopoverAlign::Start,
            gap: 8.0,
        },
        PopoverPlacement {
            side: PopoverSide::Top,
            align: PopoverAlign::Start,
            gap: 8.0,
        },
    ]
}

fn tooltip_text(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        // Asset paths/identifiers often have no word boundaries. Do not let a
        // single long token set the flex item's minimum width beyond the popup.
        TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
        Node {
            min_width: Val::Px(0.0),
            max_width: Val::Percent(100.0),
            ..default()
        },
        Pickable::IGNORE,
    )
}

fn clear_popup(commands: &mut Commands, state: &mut TooltipState) {
    if let Some(popup) = state.popup.take() {
        commands.entity(popup).despawn();
    }
}

fn clear_tooltip(commands: &mut Commands, state: &mut TooltipState) {
    clear_popup(commands, state);
    state.target = None;
    state.hovered_at = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_unbroken_tooltip_paths_wrap_inside_the_panel_at_each_scale() {
        use bevy::{
            app::{HierarchyPropagatePlugin, PropagateSet},
            camera::{ComputedCameraValues, RenderTargetInfo},
            text::{TextLayoutInfo, TextPlugin},
            ui::{
                ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system,
                ui_surface::UiSurface,
                update::propagate_ui_target_cameras,
                widget::{measure_text_system, text_system},
            },
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TextPlugin,
            HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
            HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
        ))
        .init_asset::<Image>()
        .init_resource::<UiScale>()
        .init_resource::<UiSurface>()
        .add_systems(
            PostUpdate,
            (
                propagate_ui_target_cameras,
                measure_text_system,
                ui_layout_system,
                text_system,
            )
                .chain()
                .after(bevy::text::load_font_assets_into_font_collection),
        )
        .configure_sets(
            PostUpdate,
            PropagateSet::<ComputedUiTargetCamera>::default()
                .after(propagate_ui_target_cameras)
                .before(measure_text_system),
        )
        .configure_sets(
            PostUpdate,
            PropagateSet::<ComputedUiRenderTargetInfo>::default()
                .after(propagate_ui_target_cameras)
                .before(measure_text_system),
        );
        let camera = app
            .world_mut()
            .spawn((
                Camera2d,
                Camera {
                    computed: ComputedCameraValues {
                        target_info: Some(RenderTargetInfo {
                            physical_size: UVec2::new(800, 1000),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    ..default()
                },
            ))
            .id();
        let panel = app
            .world_mut()
            .spawn((
                UiTargetCamera(camera),
                Node {
                    width: Val::Px(180.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
            ))
            .id();
        let value = format!(
            "C:\\projects\\{}\\texture.png",
            "long_asset_name_".repeat(12)
        );
        let label = app
            .world_mut()
            .spawn(tooltip_text(&value, 11.0, theme::TEXT))
            .id();
        app.world_mut().entity_mut(panel).add_child(label);
        for scale in [1.0, 1.5, 2.0] {
            for width in [180.0, 280.0] {
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                app.world_mut().get_mut::<Node>(panel).unwrap().width = Val::Px(width);
                for _ in 0..4 {
                    app.update();
                }
                let node = app.world().get::<ComputedNode>(label).unwrap();
                let layout = app.world().get::<TextLayoutInfo>(label).unwrap();
                assert_eq!(app.world().get::<Text>(label).unwrap().0, value);
                assert!(node.size().x <= (width - 20.0) * scale + 1.0);
                assert!(
                    layout.size.x <= node.size().x + 1.0,
                    "path overflows at {width}px/{scale}x: {:?} vs {:?}",
                    layout.size,
                    node.size()
                );
                assert!(
                    layout.size.y > 22.0 * scale,
                    "expected multiple visible lines"
                );
            }
        }
    }

    #[test]
    fn tooltip_defaults_to_delayed_description() {
        let tooltip = EditorTooltip::description("Particle lifetime")
            .with_shortcut("F")
            .with_footer("Hold Shift for precision");

        assert_eq!(tooltip.title, None);
        assert_eq!(tooltip.description, "Particle lifetime");
        assert_eq!(tooltip.shortcut.as_deref(), Some("F"));
        assert_eq!(tooltip.footer.as_deref(), Some("Hold Shift for precision"));
        assert_eq!(tooltip.delay, DEFAULT_TOOLTIP_DELAY);
        assert_eq!(tooltip.preferred_side, EditorTooltipSide::Left);
    }

    #[test]
    fn tooltip_automatically_tracks_relative_cursor_position() {
        let mut world = World::new();
        let entity = world
            .spawn(EditorTooltip::titled("Lifetime", "Particle lifetime"))
            .id();

        assert!(world.entity(entity).contains::<RelativeCursorPosition>());
    }

    #[test]
    fn tooltip_delay_can_be_overridden() {
        let delay = Duration::from_millis(1200);
        let tooltip = EditorTooltip::description("Slow help").with_delay(delay);

        assert_eq!(tooltip.delay, delay);
    }

    #[test]
    fn tooltip_anchor_defaults_to_element_and_can_target_the_cursor() {
        let element = EditorTooltip::titled(
            "Lower edge input [Float]",
            "Value where the smooth transition begins.",
        );
        assert_eq!(element.anchor, EditorTooltipAnchor::Element);
        // The name [type] title is carried into the accessible label so it is never dropped.
        assert!(
            element
                .accessible_label()
                .contains("Lower edge input [Float]")
        );

        let cursor = element.anchored_to_cursor();
        assert_eq!(cursor.anchor, EditorTooltipAnchor::Cursor);
    }

    #[test]
    fn tooltip_side_controls_the_first_horizontal_placement() {
        let positions = tooltip_positions(EditorTooltipSide::Right);

        assert_eq!(positions[0].side, PopoverSide::Right);
        assert_eq!(positions[1].side, PopoverSide::Left);
        assert!(
            positions
                .iter()
                .any(|placement| placement.side == PopoverSide::Bottom)
        );
        assert!(
            positions
                .iter()
                .any(|placement| placement.side == PopoverSide::Top)
        );
    }
}
