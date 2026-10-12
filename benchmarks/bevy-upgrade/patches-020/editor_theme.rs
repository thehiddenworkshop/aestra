//! Real Aestra palette and version-selected theme boundary with real Feathers controls.
#[path = "../../../apps/aestra-editor/src/theme/bevy_020.rs"]
pub mod boundary;
#[path = "../../../apps/aestra-editor/src/theme/palette.rs"]
pub mod palette;
pub use boundary::feathers_theme;
pub use palette::*;
#[path = "../../../apps/aestra-editor/src/theme/tests.rs"]
mod shared_tests;

use bevy::{
    app::Propagate,
    feathers::{
        FeathersPlugins,
        controls::{ButtonVariant, FeathersButton, FeathersMenuItem},
        dark_theme::create_dark_theme,
        theme::{SurfaceLevel, ThemeBackgroundColor, ThemeContext, ThemeTextColor, UiTheme},
        tokens,
    },
    picking::hover::Hovered,
    prelude::*,
    render::{RenderPlugin, settings::WgpuSettings},
    ui::{InteractionDisabled, Pressed},
    window::{ExitCondition, WindowPlugin},
};

const SURFACES: [SurfaceLevel; 4] = [
    SurfaceLevel::Base,
    SurfaceLevel::Higher,
    SurfaceLevel::Highest,
    SurfaceLevel::Floating,
];

#[test]
fn every_editor_color_is_exact_in_every_surface_context() {
    let theme = feathers_theme();
    for context in SURFACES {
        for (token, color) in palette::colors() {
            assert_eq!(
                theme.context_color(&token, context),
                color,
                "{context:?} {token}"
            );
        }
        // These intentional differences must not collapse into one upstream semantic fill.
        assert_ne!(
            theme.context_color(&tokens::MENUITEM_BG_HOVER, context),
            theme.context_color(&tokens::BUTTON_PLAIN_BG_HOVER, context)
        );
        assert_ne!(
            theme.context_color(&tokens::BUTTON_PRIMARY_BG, context),
            theme.context_color(&tokens::CHECKBOX_BG_CHECKED, context)
        );
    }
}

#[test]
fn unmapped_upstream_tokens_keep_complete_contextual_defaults() {
    let theme = feathers_theme();
    let upstream = create_dark_theme();
    let assigned = palette::colors();
    for token in upstream.token_assignments.keys() {
        for context in SURFACES {
            // Every assignment must resolve to a real semantic value, not the missing-token fallback.
            let semantic = &theme.0.token_assignments[token];
            assert!(theme.0.semantic_base.contains_key(semantic));
            if !assigned.contains_key(token)
                && *token != tokens::GROUP_BG
                && *token != tokens::GROUP_BORDER
            {
                assert_eq!(
                    theme.context_color(token, context),
                    upstream.context_color(token, context)
                );
            }
        }
    }
}

#[test]
fn merged_groups_preserve_body_fill_and_border_in_every_context() {
    let theme = feathers_theme();
    for context in SURFACES {
        assert_eq!(theme.context_color(&tokens::GROUP_BG, context), PANEL);
        assert_eq!(theme.context_color(&tokens::GROUP_BORDER, context), BORDER);
    }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            // Register real shader/assets dependencies without creating a GPU or window.
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    backends: None,
                    ..default()
                }
                .into(),
                ..default()
            })
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(FeathersPlugins)
    // This gate drives control state directly; pointer hit testing belongs to the
    // later input-compatibility gate. Do not let an empty headless hover map reset it.
    .insert_resource(bevy::picking::PickingSettings {
        is_hover_enabled: false,
        ..default()
    })
    .insert_resource(feathers_theme());
    app.finish();
    app.cleanup();
    app
}

fn update(app: &mut App) {
    // Allow actual theme context and inherited text propagation to settle.
    for _ in 0..3 {
        app.update();
    }
}

fn spawn(app: &mut App, scene: impl Scene) -> Entity {
    let mut entity = app.world_mut().spawn_empty();
    entity.apply_scene(scene).unwrap();
    entity.id()
}

#[test]
fn actual_button_styles_preserve_hover_press_disable_and_reenable() {
    let mut app = app();
    for context in SURFACES {
        let button = spawn(
            &mut app,
            bsn! {
                @FeathersButton
                ThemeContext(context)
            },
        );
        update(&mut app);
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            BUTTON
        );
        app.world_mut().entity_mut(button).insert(Hovered(true));
        update(&mut app);
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            BUTTON_HOVER
        );
        app.world_mut().entity_mut(button).insert(Pressed);
        update(&mut app);
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            ACCENT_DIM
        );
        app.world_mut()
            .entity_mut(button)
            .insert(InteractionDisabled);
        update(&mut app);
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            PANEL_LIGHT
        );
        app.world_mut()
            .entity_mut(button)
            .remove::<(InteractionDisabled, Pressed)>();
        update(&mut app);
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            BUTTON_HOVER
        );
        app.world_mut()
            .entity_mut(button)
            .insert(ButtonVariant::Primary);
        update(&mut app);
        assert_eq!(
            app.world().get::<BackgroundColor>(button).unwrap().0,
            Color::srgb(0.43, 0.31, 0.82)
        );
        app.world_mut().entity_mut(button).despawn();
    }
}

#[test]
fn actual_menu_hover_and_keyboard_focus_preserve_contrast_in_floating_context() {
    let mut app = app();
    let item = spawn(
        &mut app,
        bsn! {
            @FeathersMenuItem
            ThemeContext(SurfaceLevel::Floating)
        },
    );
    update(&mut app);
    app.world_mut().entity_mut(item).insert(Hovered(true));
    update(&mut app);
    assert_eq!(
        app.world().get::<BackgroundColor>(item).unwrap().0,
        MENU_ITEM_HOVER
    );
    app.world_mut().entity_mut(item).insert(Hovered(false));
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(item, bevy::input_focus::FocusCause::Navigated);
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocusVisible>()
        .0 = true;
    update(&mut app);
    assert_eq!(
        app.world().get::<BackgroundColor>(item).unwrap().0,
        MENU_ITEM_HOVER
    );
    app.world_mut().entity_mut(item).insert(InteractionDisabled);
    update(&mut app);
    let token = &app
        .world()
        .get::<bevy::feathers::theme::InheritableThemeTextColor>(item)
        .unwrap()
        .0;
    assert_eq!(*token, tokens::MENUITEM_TEXT_DISABLED);
    assert_eq!(
        app.world()
            .resource::<UiTheme>()
            .context_color(token, SurfaceLevel::Floating),
        TEXT_FAINT
    );
}

#[test]
fn inherited_context_changes_and_theme_replacement_refresh_real_components() {
    let mut app = app();
    let parent = app
        .world_mut()
        .spawn((Node::default(), Propagate(ThemeContext(SurfaceLevel::Base))))
        .id();
    let child = app
        .world_mut()
        .spawn((
            Node::default(),
            ThemeBackgroundColor(tokens::BUTTON_BG_HOVER),
            ChildOf(parent),
        ))
        .id();
    let text = app
        .world_mut()
        .spawn((
            Text::new("Disabled"),
            ThemeTextColor(tokens::MENUITEM_TEXT_DISABLED),
            ChildOf(parent),
        ))
        .id();
    update(&mut app);
    app.world_mut()
        .entity_mut(parent)
        .insert(Propagate(ThemeContext(SurfaceLevel::Highest)));
    update(&mut app);
    assert_eq!(
        app.world().get::<ThemeContext>(child).unwrap().0,
        SurfaceLevel::Highest
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(child).unwrap().0,
        BUTTON_HOVER
    );
    assert_eq!(app.world().get::<TextColor>(text).unwrap().0, TEXT_FAINT);
    let mut changed = feathers_theme();
    let semantic = changed.0.token_assignments[&tokens::BUTTON_BG_HOVER].clone();
    changed.0.semantic_base.insert(semantic, ACCENT);
    app.insert_resource(changed);
    update(&mut app);
    assert_eq!(app.world().get::<BackgroundColor>(child).unwrap().0, ACCENT);
    app.insert_resource(feathers_theme());
    update(&mut app);
    assert_eq!(
        app.world().get::<BackgroundColor>(child).unwrap().0,
        BUTTON_HOVER
    );
}
