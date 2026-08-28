//! Remembered collapsible cards for editor property panels.

use crate::{
    feathers::{button::FeathersActionButton, scenes as ui_shell, tooltip::EditorTooltip},
    theme,
};
use bevy::{
    feathers::{constants::icons, display::icon, theme::ThemedText},
    prelude::*,
    ui_widgets::Activate,
};
use std::collections::BTreeMap;

/// Stable expansion policy for one semantic kind of panel card.
///
/// The key belongs to the editor document schema rather than the transient ECS entity that renders
/// the card. Rebuilt cards and different instances of the same semantic kind therefore share the
/// user's remembered preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RememberedPanelCard {
    key: String,
    default_expanded: bool,
}

impl RememberedPanelCard {
    pub(crate) fn new(key: impl Into<String>, default_expanded: bool) -> Self {
        Self {
            key: key.into(),
            default_expanded,
        }
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn expanded(&self, memory: &BTreeMap<String, bool>) -> bool {
        memory
            .get(self.key())
            .copied()
            .unwrap_or(self.default_expanded)
    }

    pub(crate) fn collapsed(&self, memory: &BTreeMap<String, bool>) -> bool {
        !self.expanded(memory)
    }

    pub(crate) fn toggle(&self, memory: &mut BTreeMap<String, bool>) -> bool {
        let expanded = !self.expanded(memory);
        memory.insert(self.key.clone(), expanded);
        expanded
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PanelCardProps<'a> {
    pub(crate) title: &'a str,
    pub(crate) help: Option<&'a str>,
    pub(crate) memory_key: Option<String>,
    pub(crate) collapsed: bool,
    pub(crate) enabled: bool,
    pub(crate) background: Color,
    pub(crate) border: Color,
}

impl<'a> PanelCardProps<'a> {
    pub(crate) fn new(title: &'a str, collapsed: bool) -> Self {
        Self {
            title,
            help: None,
            memory_key: None,
            collapsed,
            enabled: true,
            background: theme::PANEL_LIGHT,
            border: theme::BORDER,
        }
    }

    pub(crate) fn with_help(mut self, help: &'a str) -> Self {
        self.help = Some(help);
        self
    }

    pub(crate) fn with_memory_key(mut self, key: impl Into<String>) -> Self {
        self.memory_key = Some(key.into());
        self
    }

    pub(crate) fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub(crate) fn with_background(mut self, background: Color) -> Self {
        self.background = background;
        self
    }

    pub(crate) fn with_border(mut self, border: Color) -> Self {
        self.border = border;
        self
    }
}

#[derive(Component, Debug, Clone)]
struct PanelCardToggle {
    card: Entity,
    body: Entity,
    icon: Entity,
    title: String,
    memory_key: Option<String>,
    collapsed: bool,
}

fn apply_collapsed_visuals(
    toggle: &PanelCardToggle,
    nodes: &mut Query<&mut Node>,
    transforms: &mut Query<&mut UiTransform>,
) {
    if let Ok(mut card) = nodes.get_mut(toggle.card) {
        card.padding = UiRect::axes(
            Val::Px(6.0),
            Val::Px(if toggle.collapsed { 3.0 } else { 5.0 }),
        );
        card.row_gap = Val::Px(if toggle.collapsed { 0.0 } else { 2.0 });
    }
    if let Ok(mut body) = nodes.get_mut(toggle.body) {
        body.display = if toggle.collapsed {
            Display::None
        } else {
            Display::Flex
        };
    }
    if let Ok(mut icon) = transforms.get_mut(toggle.icon) {
        icon.rotation = if toggle.collapsed {
            Rot2::radians(-std::f32::consts::FRAC_PI_2)
        } else {
            Rot2::IDENTITY
        };
    }
}

fn toggle_panel_card(
    activate: On<Activate>,
    mut commands: Commands,
    mut toggles: Query<(Entity, &mut PanelCardToggle)>,
    mut nodes: Query<&mut Node>,
    mut transforms: Query<&mut UiTransform>,
) {
    let (memory_key, collapsed) = {
        let Ok((_, toggle)) = toggles.get(activate.entity) else {
            return;
        };
        (toggle.memory_key.clone(), !toggle.collapsed)
    };

    for (button, mut toggle) in &mut toggles {
        let matches = button == activate.entity
            || memory_key
                .as_ref()
                .is_some_and(|key| toggle.memory_key.as_ref() == Some(key));
        if !matches {
            continue;
        }
        toggle.collapsed = collapsed;
        apply_collapsed_visuals(&toggle, &mut nodes, &mut transforms);
        commands.entity(button).insert(AccessibleLabel(format!(
            "{} {}",
            if toggle.collapsed {
                "Expand"
            } else {
                "Collapse"
            },
            toggle.title
        )));
    }
}

/// Spawn a compact collapsible card with domain-owned header actions and body content.
///
/// `root_bundle` and `header_bundle` retain semantic selection/diagnostic ownership in the panel
/// plugin. `toggle_action` is the semantic command emitted by the disclosure button. The shared
/// widget owns only visual composition, accessibility, and disclosure behavior.
pub(crate) fn spawn_panel_card<RootBundle, HeaderBundle, ToggleBundle, HeaderActions, Body>(
    parent: &mut ChildSpawnerCommands,
    props: PanelCardProps<'_>,
    root_bundle: RootBundle,
    header_bundle: HeaderBundle,
    toggle_action: ToggleBundle,
    spawn_header_actions: HeaderActions,
    spawn_body: Body,
) where
    RootBundle: Bundle,
    HeaderBundle: Bundle,
    ToggleBundle: Bundle,
    HeaderActions: FnOnce(&mut ChildSpawnerCommands),
    Body: FnOnce(&mut ChildSpawnerCommands),
{
    let collapsed = props.collapsed;
    let mut card = parent.spawn((
        root_bundle,
        Node {
            width: Val::Auto,
            margin: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(if collapsed { 3.0 } else { 5.0 })),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(if collapsed { 0.0 } else { 2.0 }),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(props.background),
        BorderColor::all(props.border),
    ));
    let card_entity = card.id();
    card.with_children(|card| {
        let mut disclosure_entities = None;
        card.spawn((
            header_bundle,
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|header| {
            disclosure_entities = Some(spawn_disclosure(header, &props, toggle_action));
            spawn_header_actions(header);
        });
        let body = card
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                display: if collapsed {
                    Display::None
                } else {
                    Display::Flex
                },
                ..default()
            })
            .with_children(spawn_body)
            .id();
        let (disclosure, icon) =
            disclosure_entities.expect("panel card disclosure should be spawned");
        card.commands().entity(disclosure).insert(PanelCardToggle {
            card: card_entity,
            body,
            icon,
            title: props.title.to_owned(),
            memory_key: props.memory_key.clone(),
            collapsed,
        });
    });
}

fn spawn_disclosure(
    parent: &mut ChildSpawnerCommands,
    props: &PanelCardProps<'_>,
    toggle_action: impl Bundle,
) -> (Entity, Entity) {
    let mut disclosure = parent.spawn_empty();
    let disclosure_entity = disclosure.id();
    disclosure
        .apply_scene(ui_shell::feathers_plain_button())
        .insert((
            toggle_action,
            FeathersActionButton,
            AccessibleLabel(format!(
                "{} {}",
                if props.collapsed {
                    "Expand"
                } else {
                    "Collapse"
                },
                props.title
            )),
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                height: Val::Px(26.0),
                padding: UiRect::horizontal(Val::Px(2.0)),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .observe(toggle_panel_card);
    if let Some(help) = props.help {
        disclosure.insert(EditorTooltip::titled(props.title, help));
    }
    let mut icon_entity = None;
    disclosure.with_children(|button| {
        let mut disclosure_icon = button.spawn_empty();
        icon_entity = Some(disclosure_icon.id());
        disclosure_icon
            .apply_scene(icon(icons::CHEVRON_DOWN))
            .insert((
                Pickable::IGNORE,
                UiTransform::from_rotation(if props.collapsed {
                    Rot2::radians(-std::f32::consts::FRAC_PI_2)
                } else {
                    Rot2::IDENTITY
                }),
            ));
        button.spawn((
            Text::new(props.title),
            ThemedText,
            TextColor(if props.enabled {
                theme::TEXT
            } else {
                theme::TEXT_FAINT
            }),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            Pickable::IGNORE,
        ));
    });
    (
        disclosure_entity,
        icon_entity.expect("panel card disclosure icon should be spawned"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_test_card(world: &mut World, key: &str) -> (Entity, Entity, Entity, Entity) {
        let card = world.spawn(Node::default()).id();
        let body = world.spawn(Node::default()).id();
        let icon = world.spawn(UiTransform::IDENTITY).id();
        let button = world
            .spawn((
                PanelCardToggle {
                    card,
                    body,
                    icon,
                    title: "Motion".into(),
                    memory_key: Some(key.into()),
                    collapsed: false,
                },
                AccessibleLabel("Collapse Motion".into()),
            ))
            .id();
        (card, body, icon, button)
    }

    #[test]
    fn remembered_card_uses_default_until_the_user_toggles_it() {
        let mut memory = BTreeMap::new();
        let card = RememberedPanelCard::new("module/aestra.update.motion", false);

        assert!(card.collapsed(&memory));
        assert!(card.toggle(&mut memory));
        assert!(!card.collapsed(&memory));
        assert_eq!(memory.get(card.key()), Some(&true));
    }

    #[test]
    fn cards_with_the_same_semantic_key_share_memory() {
        let mut memory = BTreeMap::new();
        let first = RememberedPanelCard::new("renderer/sprite", false);
        let rebuilt = RememberedPanelCard::new("renderer/sprite", false);

        first.toggle(&mut memory);

        assert!(rebuilt.expanded(&memory));
    }

    #[test]
    fn different_semantic_keys_keep_independent_preferences() {
        let mut memory = BTreeMap::new();
        let emission = RememberedPanelCard::new("module/emission", true);
        let motion = RememberedPanelCard::new("module/motion", false);

        emission.toggle(&mut memory);

        assert!(!emission.expanded(&memory));
        assert!(!motion.expanded(&memory));
        assert_eq!(memory.len(), 1);
    }

    #[test]
    fn disclosure_toggles_existing_card_bodies_without_rebuilding() {
        let mut app = App::new();
        app.add_observer(toggle_panel_card);
        let first = spawn_test_card(app.world_mut(), "module/motion");
        let second = spawn_test_card(app.world_mut(), "module/motion");

        app.world_mut().trigger(Activate { entity: first.3 });

        for (card, body, icon, button) in [first, second] {
            assert!(app.world().entities().contains(card));
            assert_eq!(
                app.world().get::<Node>(body).unwrap().display,
                Display::None
            );
            assert_eq!(
                app.world().get::<UiTransform>(icon).unwrap().rotation,
                Rot2::radians(-std::f32::consts::FRAC_PI_2)
            );
            assert!(
                app.world()
                    .get::<PanelCardToggle>(button)
                    .unwrap()
                    .collapsed
            );
        }
    }
}
