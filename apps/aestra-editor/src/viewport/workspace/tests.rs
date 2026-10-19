//! Real preview panel, toolbar activation, SVG handles and status consumers on both engines.
use super::super::{
    actions::{
        PreviewDisplayState, audit_viewport_controls, execute_viewport_action,
        handle_viewport_action_buttons, queue_viewport_action_activation,
    },
    navigation::PreviewCameraController,
    presentation::{update_preview_display_controls, update_transform_gizmo_controls},
};
use super::*;
use crate::{
    docking::{MaximizedPanel, ToolPanel},
    menus::MenuState,
    settings::{EditorSettings, SettingsPersistence},
    test_support,
};
use bevy::{
    asset::AssetPlugin,
    gizmos::transform_gizmo::TransformGizmoSettings,
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{Activate, Button as NativeButton},
};
#[derive(Resource)]
struct Built(Entity);
#[derive(Resource)]
struct Maximize(bool);
fn build(
    mut commands: Commands,
    localizer: Res<Localizer>,
    assets: Res<AssetServer>,
    max: Res<Maximize>,
) {
    let root = commands
        .spawn(Node::default())
        .with_children(|p| spawn_preview(p, max.0, &localizer, &assets))
        .id();
    commands.insert_resource(Built(root));
}
struct Fixture {
    app: App,
    _root: tempfile::TempDir,
}
impl Fixture {
    fn new(locale: &str, maximized: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .add_plugins(crate::feathers::icon::SvgPlugin)
        .insert_resource(test_support::session_with_timing_slack())
        .insert_resource(Localizer::new(locale).unwrap())
        .insert_resource(Maximize(maximized))
        .init_resource::<MenuState>()
        .init_resource::<EditorSettings>()
        .insert_resource(SettingsPersistence::for_test(
            root.path().join("settings.ron"),
        ))
        .init_resource::<PreviewCameraController>()
        .init_resource::<PreviewDisplayState>()
        .init_resource::<TransformGizmoSettings>()
        .init_resource::<MaximizedPanel>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EditorPreviewProject>()
        .add_observer(queue_viewport_action_activation)
        .add_observer(execute_viewport_action)
        .add_systems(Startup, build)
        .add_systems(
            Update,
            (
                handle_viewport_action_buttons,
                audit_viewport_controls,
                update_viewport_status_label,
                update_preview_display_controls,
                update_transform_gizmo_controls,
            )
                .chain(),
        );
        app.update();
        Self { app, _root: root }
    }
    fn action(&mut self, action: ViewportAction) -> Entity {
        self.app
            .world_mut()
            .query::<(Entity, &ViewportAction)>()
            .iter(self.app.world())
            .find(|(_, a)| **a == action)
            .unwrap()
            .0
    }
    fn activate(&mut self, action: ViewportAction) {
        let entity = self.action(action);
        self.app.world_mut().trigger(Activate { entity });
        self.app.update();
    }
    fn status(&mut self) -> String {
        self.app
            .world_mut()
            .query_filtered::<&Text, With<ParticleCountLabel>>()
            .single(self.app.world())
            .unwrap()
            .0
            .clone()
    }
}
#[test]
fn actual_viewport_pane_and_canvas_preserve_layout_and_clipping() {
    let mut f = Fixture::new("en-US", false);
    let root = f.app.world().resource::<Built>().0;
    let column = f.app.world().get::<Children>(root).unwrap()[0];
    let node = f.app.world().get::<Node>(column).unwrap();
    assert_eq!(node.height, Val::Percent(100.0));
    assert_eq!(node.flex_grow, 1.0);
    assert_eq!(node.padding, UiRect::all(Val::Px(12.0)));
    assert_eq!(node.row_gap, Val::Px(8.0));
    assert_eq!(
        f.app.world().get::<BackgroundColor>(column).unwrap().0,
        Color::NONE
    );
    let w = f.app.world_mut();
    let (e, node, _) = w
        .query_filtered::<(Entity, &Node, &RelativeCursorPosition), With<PreviewCanvas>>()
        .single(w)
        .unwrap();
    assert_eq!(node.min_height, Val::Px(180.0));
    assert_eq!(node.overflow, Overflow::clip());
    assert_eq!(node.border_radius, BorderRadius::all(Val::Px(6.0)));
    assert_eq!(w.get::<ChildOf>(e).unwrap().parent(), column);
}
#[test]
fn panel_builds_seven_actual_classified_feathers_controls_and_svg_icons() {
    let mut f = Fixture::new("en-US", false);
    let w = f.app.world_mut();
    let actions = w
        .query::<(Entity, &ViewportAction, &Node)>()
        .iter(w)
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 7);
    for (e, _, node) in actions {
        assert!(w.get::<NativeButton>(e).is_some());
        assert!(w.get::<FeathersActionButton>(e).is_some());
        assert!(w.get::<AccessibleLabel>(e).is_some());
        assert!(w.get::<EditorTooltip>(e).is_some());
        assert_eq!(node.width, Val::Px(28.0));
        assert_eq!(node.height, Val::Px(28.0));
    }
    assert_eq!(w.query::<&UiSvg>().iter(w).count(), 7);
}
#[test]
fn localized_gizmo_tooltips_keep_titles_descriptions_and_shortcuts() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::new(locale, false);
        for (mode, key, description, shortcut) in [
            (
                TransformGizmoMode::Translate,
                "viewport-gizmo-move",
                "viewport-gizmo-move-description",
                "1",
            ),
            (
                TransformGizmoMode::Rotate,
                "viewport-gizmo-rotate",
                "viewport-gizmo-rotate-description",
                "2",
            ),
            (
                TransformGizmoMode::Scale,
                "viewport-gizmo-scale",
                "viewport-gizmo-scale-description",
                "3",
            ),
        ] {
            let e = f.action(ViewportAction::SetTransformGizmoMode(mode));
            let l = f.app.world().resource::<Localizer>();
            let title = l.text(key);
            let desc = l.text(description);
            assert_eq!(f.app.world().get::<AccessibleLabel>(e).unwrap().0, title);
            assert_eq!(
                f.app
                    .world()
                    .get::<EditorTooltip>(e)
                    .unwrap()
                    .accessible_label(),
                format!("{title}. {desc}. {shortcut}")
            );
        }
    }
}
#[test]
fn maximize_state_selects_the_actual_initial_fullscreen_icon_and_localized_label() {
    for locale in ["en-US", "fr-FR"] {
        for max in [false, true] {
            let mut f = Fixture::new(locale, max);
            let e = f.action(ViewportAction::ToggleMaximize);
            let expected = f.app.world().resource::<Localizer>().text(if max {
                "viewport-reduce"
            } else {
                "viewport-enlarge"
            });
            assert_eq!(f.app.world().get::<AccessibleLabel>(e).unwrap().0, expected);
            let child = f.app.world().get::<Children>(e).unwrap()[0];
            let icon = f.app.world().get::<UiSvg>(child).unwrap();
            let path = f
                .app
                .world()
                .resource::<AssetServer>()
                .get_path(icon.0.id())
                .unwrap();
            assert_eq!(
                path.path().to_string_lossy(),
                if max {
                    "icons/fullscreen-exit-alt.svg"
                } else {
                    "icons/fullscreen-alt.svg"
                }
            );
        }
    }
}
#[test]
fn svg_children_keep_original_asset_paths_size_tint_and_nonpicking_contract() {
    let mut f = Fixture::new("en-US", false);
    for (action, path) in [
        (ViewportAction::FramePreview, "icons/center-focus.svg"),
        (
            ViewportAction::SetPreviewDisplayMode(PreviewDisplayMode::Wireframe),
            "icons/wireframe.svg",
        ),
        (
            ViewportAction::SetPreviewDisplayMode(PreviewDisplayMode::Rendered),
            "icons/solid.svg",
        ),
        (
            ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Translate),
            "icons/move.svg",
        ),
        (
            ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Rotate),
            "icons/rotate.svg",
        ),
        (
            ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Scale),
            "icons/scale.svg",
        ),
    ] {
        let e = f.action(action);
        let child = f.app.world().get::<Children>(e).unwrap()[0];
        let icon = f.app.world().get::<UiSvg>(child).unwrap();
        assert_eq!(
            f.app
                .world()
                .resource::<AssetServer>()
                .get_path(icon.0.id())
                .unwrap()
                .path()
                .to_string_lossy(),
            path
        );
        assert_eq!(
            f.app.world().get::<Node>(child).unwrap().width,
            Val::Px(20.0)
        );
        assert_eq!(
            f.app.world().get::<Node>(child).unwrap().height,
            Val::Px(20.0)
        );
        assert_eq!(
            *f.app.world().get::<Pickable>(child).unwrap(),
            Pickable::IGNORE
        );
        assert!(f.app.world().get::<SvgColor>(child).is_some());
    }
}
#[test]
fn both_drag_readouts_start_hidden_nonpicking_and_themed() {
    let mut f = Fixture::new("en-US", false);
    let w = f.app.world_mut();
    let labels=w.query_filtered::<(Entity,&Node,&Text,&Visibility),Or<(With<ShapeGizmoValueLabel>,With<TransformGizmoValueLabel>)>>().iter(w).collect::<Vec<_>>();
    assert_eq!(labels.len(), 2);
    for (e, node, text, visibility) in labels {
        assert!(text.0.is_empty());
        assert_eq!(*visibility, Visibility::Hidden);
        assert_eq!(node.position_type, PositionType::Absolute);
        assert_eq!(*w.get::<Pickable>(e).unwrap(), Pickable::IGNORE);
        assert_eq!(w.get::<TextColor>(e).unwrap().0, theme::TEXT);
        assert_eq!(
            w.get::<BorderColor>(e).unwrap(),
            &BorderColor::all(theme::ACCENT_DIM)
        );
    }
}
#[test]
fn real_mode_activation_updates_retained_label_and_icon_without_semantic_edits() {
    let mut f = Fixture::new("en-US", false);
    let s = f.app.world().resource::<EditorSession>();
    let before = s.effect.clone();
    let revision = s.document_revision();
    let entity_count = f.app.world().entities().len();
    let target = f.action(ViewportAction::SetTransformGizmoMode(
        TransformGizmoMode::Rotate,
    ));
    f.activate(ViewportAction::SetTransformGizmoMode(
        TransformGizmoMode::Rotate,
    ));
    assert_eq!(
        f.app.world().resource::<TransformGizmoSettings>().mode,
        TransformGizmoMode::Rotate
    );
    let w = f.app.world_mut();
    assert_eq!(
        w.query_filtered::<&Text, With<GizmoModeLabel>>()
            .single(w)
            .unwrap()
            .0,
        "ROTATE · WORLD"
    );
    for (icon, color) in w.query::<(&TransformGizmoModeIcon, &SvgColor)>().iter(w) {
        assert_eq!(
            color.0,
            if icon.0 == TransformGizmoMode::Rotate {
                theme::ACCENT
            } else {
                theme::TEXT
            }
        );
    }
    assert_eq!(
        f.action(ViewportAction::SetTransformGizmoMode(
            TransformGizmoMode::Rotate
        )),
        target
    );
    assert_eq!(f.app.world().entities().len(), entity_count);
    let s = f.app.world().resource::<EditorSession>();
    assert_eq!(s.effect, before);
    assert_eq!(s.document_revision(), revision);
    assert!(!s.dirty && !s.can_undo());
}
#[test]
fn real_display_activation_recolors_existing_controls_and_consumes_once() {
    let mut f = Fixture::new("en-US", false);
    f.activate(ViewportAction::SetPreviewDisplayMode(
        PreviewDisplayMode::Wireframe,
    ));
    let w = f.app.world_mut();
    for (icon, color) in w.query::<(&PreviewDisplayModeIcon, &SvgColor)>().iter(w) {
        assert_eq!(
            color.0,
            if icon.0 == PreviewDisplayMode::Wireframe {
                theme::ACCENT
            } else {
                theme::TEXT
            }
        );
    }
    f.app
        .world_mut()
        .resource_mut::<PreviewDisplayState>()
        .set_mode(PreviewDisplayMode::Rendered);
    f.app.update();
    assert_eq!(
        f.app.world().resource::<PreviewDisplayState>().mode,
        PreviewDisplayMode::Rendered
    );
}
#[test]
fn actual_frame_and_maximize_controls_reach_their_original_consumers() {
    let mut f = Fixture::new("en-US", false);
    f.activate(ViewportAction::FramePreview);
    assert!(
        f.app
            .world()
            .resource::<PreviewCameraController>()
            .frame_requested
    );
    f.activate(ViewportAction::ToggleMaximize);
    assert_eq!(
        f.app.world().resource::<MaximizedPanel>().0,
        Some(ToolPanel::Viewport)
    );
    f.activate(ViewportAction::ToggleMaximize);
    assert_eq!(f.app.world().resource::<MaximizedPanel>().0, None);
    assert!(!f.app.world().resource::<EditorSession>().dirty);
}
fn status(active: ActiveBackend) -> EffectRuntimeStatus {
    EffectRuntimeStatus {
        active,
        reason: "fixture".into(),
        compatibility: aestra_runtime::CompatibilityReport::compatible(
            aestra_runtime::CompatibilityTarget::NativeGpu,
        ),
    }
}
#[test]
fn status_reports_all_actual_backend_variants_and_current_particle_count() {
    let mut f = Fixture::new("en-US", false);
    let e = f
        .app
        .world_mut()
        .spawn((PreviewPresentedEffect, status(ActiveBackend::Pending)))
        .id();
    f.app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .live_particle_count = 77;
    for (active, label) in [
        (ActiveBackend::Pending, "DETECTING GPU"),
        (ActiveBackend::Gpu, "NATIVE GPU"),
        (ActiveBackend::GpuReadback, "GPU READBACK"),
        (ActiveBackend::CpuReference, "CPU FALLBACK"),
    ] {
        f.app.world_mut().entity_mut(e).insert(status(active));
        f.app.update();
        assert_eq!(f.status(), format!("77 LIVE PARTICLES  |  {label}"));
    }
}
#[test]
fn no_runtime_status_and_unowned_runtime_components_do_not_invent_a_backend() {
    let mut f = Fixture::new("en-US", false);
    assert_eq!(f.status(), "0 LIVE PARTICLES  |  DETECTING GPU");
    f.app.world_mut().spawn(status(ActiveBackend::Gpu));
    f.app.update();
    assert_eq!(f.status(), "0 LIVE PARTICLES  |  DETECTING GPU");
}
#[test]
fn preview_errors_take_precedence_and_clearing_them_restores_current_status() {
    let mut f = Fixture::new("en-US", false);
    f.app
        .world_mut()
        .spawn((PreviewPresentedEffect, status(ActiveBackend::Gpu)));
    f.app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .error = Some("missing child".into());
    f.app.update();
    assert_eq!(f.status(), "PREVIEW ERROR  |  missing child");
    f.app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .error = None;
    f.app.update();
    assert_eq!(f.status(), "0 LIVE PARTICLES  |  NATIVE GPU");
}
#[test]
fn idle_status_keeps_the_original_change_gate_and_text_identity() {
    let mut f = Fixture::new("en-US", false);
    let w = f.app.world_mut();
    let e = w
        .query_filtered::<Entity, With<ParticleCountLabel>>()
        .single(w)
        .unwrap();
    f.app.world_mut().get_mut::<Text>(e).unwrap().0 = "idle sentinel".into();
    f.app.update();
    assert_eq!(f.status(), "idle sentinel");
    f.app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .live_particle_count = 3;
    f.app.update();
    assert_eq!(f.status(), "3 LIVE PARTICLES  |  DETECTING GPU");
}
#[test]
fn runtime_removal_retains_original_gate_until_session_or_project_changes() {
    let mut f = Fixture::new("en-US", false);
    let e = f
        .app
        .world_mut()
        .spawn((PreviewPresentedEffect, status(ActiveBackend::Gpu)))
        .id();
    f.app.update();
    f.app
        .world_mut()
        .entity_mut(e)
        .remove::<EffectRuntimeStatus>();
    f.app.update();
    assert_eq!(f.status(), "0 LIVE PARTICLES  |  NATIVE GPU");
    f.app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .live_particle_count = 1;
    f.app.update();
    assert_eq!(f.status(), "1 LIVE PARTICLES  |  DETECTING GPU");
}
