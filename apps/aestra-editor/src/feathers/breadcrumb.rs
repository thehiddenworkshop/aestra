//! Reusable, typography-stable breadcrumb trails for editor navigation.

use super::{
    button::FeathersActionButton,
    combo_box::{ComboOption, spawn_compact_action_menu},
    icon::spawn_breadcrumb_chevron,
    scenes,
    tooltip::EditorTooltip,
};
use bevy::prelude::*;

#[derive(Clone)]
pub(crate) struct BreadcrumbItem<A> {
    pub(crate) label: String,
    pub(crate) action: Option<A>,
}

pub(crate) struct BreadcrumbProps<'a> {
    pub(crate) height: f32,
    pub(crate) font: &'static str,
    pub(crate) font_size: f32,
    pub(crate) text_offset_y: f32,
    pub(crate) uppercase: bool,
    pub(crate) flex_grow: f32,
    pub(crate) max_ancestor_width: f32,
    pub(crate) max_current_width: f32,
    pub(crate) ancestor_color: Color,
    pub(crate) current_color: Color,
    pub(crate) compact_ancestors: bool,
    pub(crate) overflow_label: &'a str,
    pub(crate) current_tooltip: Option<&'a str>,
    pub(crate) ancestor_tooltips: bool,
}

pub(crate) fn spawn_breadcrumb<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    items: &[BreadcrumbItem<A>],
    props: BreadcrumbProps<'_>,
    asset_server: &AssetServer,
) -> Entity {
    let mut root = parent.spawn(Node {
        min_width: Val::Px(0.0),
        height: Val::Px(props.height),
        flex_grow: props.flex_grow,
        align_items: AlignItems::Center,
        overflow: Overflow::clip(),
        ..default()
    });
    let root_entity = root.id();
    root.with_children(|trail| {
        let Some(current_index) = items.len().checked_sub(1) else {
            return;
        };
        if props.compact_ancestors && current_index > 2 {
            spawn_crumb(trail, &items[0], false, &props, asset_server);
            spawn_breadcrumb_chevron(trail, asset_server, ());
            let hidden = items[1..current_index - 1]
                .iter()
                .filter_map(|item| {
                    item.action.map(|action| ComboOption {
                        label: item.label.clone(),
                        selected: false,
                        action,
                    })
                })
                .collect::<Vec<_>>();
            trail
                .spawn(Node {
                    height: Val::Px(props.height),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|overflow| {
                    spawn_compact_action_menu(overflow, props.overflow_label, &hidden);
                });
            spawn_breadcrumb_chevron(trail, asset_server, ());
            spawn_crumb(
                trail,
                &items[current_index - 1],
                false,
                &props,
                asset_server,
            );
        } else {
            for (index, item) in items[..current_index].iter().enumerate() {
                if index > 0 {
                    spawn_breadcrumb_chevron(trail, asset_server, ());
                }
                spawn_crumb(trail, item, false, &props, asset_server);
            }
        }
        if current_index > 0 {
            spawn_breadcrumb_chevron(trail, asset_server, ());
        }
        spawn_crumb(trail, &items[current_index], true, &props, asset_server);
    });
    root_entity
}

fn spawn_crumb<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    item: &BreadcrumbItem<A>,
    current: bool,
    props: &BreadcrumbProps<'_>,
    asset_server: &AssetServer,
) {
    let max_width = if current {
        props.max_current_width
    } else {
        props.max_ancestor_width
    };
    if let Some(action) = item.action {
        let mut button = parent.spawn_empty();
        button.apply_scene(scenes::feathers_plain_button()).insert((
            action,
            FeathersActionButton,
            AccessibleLabel(item.label.clone()),
            Node {
                min_width: Val::Px(0.0),
                max_width: Val::Px(max_width),
                height: Val::Px(props.height),
                padding: UiRect::horizontal(Val::Px(3.0)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..default()
            },
        ));
        if props.ancestor_tooltips {
            button.insert(EditorTooltip::description(&item.label));
        }
        button.with_children(|button| {
            spawn_crumb_label(
                button,
                &item.label,
                props.ancestor_color,
                props,
                asset_server,
            );
        });
    } else {
        let mut label = parent.spawn(Node {
            min_width: Val::Px(0.0),
            max_width: Val::Px(max_width),
            height: Val::Px(props.height),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..default()
        });
        if current && let Some(tooltip) = props.current_tooltip {
            label.insert(EditorTooltip::description(tooltip));
        }
        label.with_children(|label| {
            spawn_crumb_label(
                label,
                &item.label,
                if current {
                    props.current_color
                } else {
                    props.ancestor_color
                },
                props,
                asset_server,
            );
        });
    }
}

fn spawn_crumb_label(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    color: Color,
    props: &BreadcrumbProps<'_>,
    asset_server: &AssetServer,
) {
    parent.spawn((
        Text::new(if props.uppercase {
            label.to_uppercase()
        } else {
            label.to_owned()
        }),
        TextFont {
            font: asset_server.load(props.font).into(),
            font_size: FontSize::Px(props.font_size),
            ..default()
        },
        TextColor(color),
        TextLayout::no_wrap(),
        UiTransform {
            translation: Val2::px(0.0, props.text_offset_y),
            ..default()
        },
        Pickable::IGNORE,
    ));
}
