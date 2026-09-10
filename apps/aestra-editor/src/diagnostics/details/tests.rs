use super::*;

#[test]
fn summaries_bound_unicode_and_unbroken_paths_without_changing_short_messages() {
    assert!(summary("message", 0).is_empty());
    assert_eq!(
        summary("Open failed: source missing", 96),
        "Open failed: source missing"
    );
    assert_eq!(summary("a\r\n  b\tc", 96), "a b c");
    let message = format!("Missing: C:/{}", "é界🦀".repeat(300));
    let compact = summary(&message, 96);
    assert_eq!(compact.chars().count(), 96);
    assert!(compact.ends_with('…'));
    assert!(message.starts_with(compact.trim_end_matches('…')));
}

#[test]
fn status_details_visibility_tracks_empty_messages_without_rebuilding() {
    let mut app = App::new();
    let mut session = test_support::session_with_timing_slack();
    session.status.clear();
    let revision = session.ui_revision;
    app.insert_resource(session)
        .add_systems(Update, sync_status_details);
    let button = app
        .world_mut()
        .spawn((DetailsAction::LatestStatus, Node::default()))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<Node>(button).unwrap().display,
        Display::None
    );
    app.world_mut().resource_mut::<EditorSession>().status = "Open failed".into();
    app.update();
    assert_eq!(
        app.world().get::<Node>(button).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision
    );
}

#[test]
fn details_snapshot_latest_status_and_back_preserves_filter_and_document() {
    let mut app = App::new();
    let session = test_support::session_with_timing_slack();
    let effect = session.effect.clone();
    let mut layout = WorkspaceLayout::default();
    // Keep the test in-memory: the already-visible panel needs no layout save.
    layout.show(ToolPanel::Diagnostics);
    app.insert_resource(session)
        .insert_resource(layout)
        .insert_resource(DiagnosticsPanelState {
            filter: DiagnosticsFilter::Warnings,
            details: None,
        })
        .add_observer(activate_details);
    let latest = app.world_mut().spawn(DetailsAction::LatestStatus).id();
    let back = app.world_mut().spawn(DetailsAction::Back).id();
    let message = format!("Missing source\nC:/{}", "long-path/".repeat(200));
    app.world_mut().resource_mut::<EditorSession>().status = message.clone();
    app.world_mut().trigger(Activate { entity: latest });
    assert_eq!(
        app.world()
            .resource::<DiagnosticsPanelState>()
            .details
            .as_deref(),
        Some(message.as_str())
    );
    let revision = app.world().resource::<EditorSession>().ui_revision;
    app.world_mut().resource_mut::<EditorSession>().status = "Saved".into();
    assert_eq!(
        app.world()
            .resource::<DiagnosticsPanelState>()
            .details
            .as_deref(),
        Some(message.as_str())
    );
    // An already visible panel must rebuild when a different detail is requested.
    app.world_mut().trigger(Activate { entity: latest });
    assert!(app.world().resource::<EditorSession>().ui_revision > revision);
    assert_eq!(
        app.world()
            .resource::<DiagnosticsPanelState>()
            .details
            .as_deref(),
        Some("Saved")
    );
    app.world_mut().trigger(Activate { entity: back });
    let state = app.world().resource::<DiagnosticsPanelState>();
    assert!(state.details.is_none());
    assert_eq!(state.filter, DiagnosticsFilter::Warnings);
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
}

fn layout_app(width: u32, scale: f32, message: &str, full: bool) -> App {
    use bevy::{
        app::{HierarchyPropagatePlugin, PropagateSet},
        camera::{ComputedCameraValues, RenderTargetInfo, Viewport},
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
        bevy::asset::AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
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
    let size = UVec2::new((width as f32 * scale) as u32, (140.0 * scale) as u32);
    let window = app.world_mut().spawn(Window::default()).id();
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window)),
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: size,
                        scale_factor: scale,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: size,
                    ..default()
                }),
                ..default()
            },
        ))
        .id();
    let localizer = Localizer::new("en-US").unwrap();
    app.world_mut()
        .commands()
        .spawn((
            Node {
                width: Val::Px(width as f32),
                height: Val::Px(140.0),
                ..default()
            },
            UiTargetCamera(camera),
        ))
        .with_children(|root| {
            if full {
                spawn_workspace(root, message, &localizer);
            } else {
                spawn_summary(root, message, &localizer);
            }
        });
    app.world_mut().flush();
    for _ in 0..4 {
        app.update();
    }
    app
}

#[test]
fn long_details_wrap_inside_narrow_viewports_and_overflow_vertically() {
    let message = format!(
        "Unavailable material\nC:/{}\nDuplicate source",
        "very_long_é_path/".repeat(100)
    );
    for width in [160, 240, 400] {
        for scale in [1.0, 2.0] {
            let mut app = layout_app(width, scale, &message, true);
            let world = app.world_mut();
            let viewport = world
                .query_filtered::<&ComputedNode, With<ScrollArea>>()
                .single(world)
                .unwrap();
            let viewport_size = viewport.size();
            assert!(viewport_size.x > 0.0 && viewport_size.x <= width as f32 * scale);
            assert!(viewport_size.y > 0.0 && viewport_size.y <= 140.0 * scale);
            assert!(
                viewport.content_size().y > viewport_size.y,
                "details must scroll"
            );
            let (text, node) = world
                .query::<(&Text, &ComputedNode)>()
                .iter(world)
                .find(|(text, _)| text.0 == message)
                .expect("full diagnostic retained");
            assert_eq!(text.0, message);
            assert!(
                node.size().x <= viewport_size.x,
                "text must wrap rather than widen viewport"
            );
            assert!(node.size().y > viewport_size.y);
        }
    }
}

#[test]
fn inline_summary_is_bounded_but_details_action_retains_the_full_message() {
    let message = "Missing source: é界/path/".repeat(100);
    let mut app = layout_app(160, 1.0, &message, false);
    let world = app.world_mut();
    let (text, node) = world
        .query::<(&Text, &ComputedNode)>()
        .iter(world)
        .find(|(text, _)| text.0.ends_with('…'))
        .unwrap();
    assert_eq!(text.0.chars().count(), 160);
    assert!(node.size().x <= 160.0);
    let action = world.query::<&DetailsAction>().single(world).unwrap();
    assert!(matches!(action, DetailsAction::Message(full) if full == &message));
}
