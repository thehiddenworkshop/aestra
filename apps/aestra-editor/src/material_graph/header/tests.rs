//! Actual Feathers header composition; full graph canvas and native picking remain separate.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::{
        action_activation::{FeathersActionButton, PendingFeathersActivation},
        icon::{SvgPlugin, UiSvg},
        text_input::tests::app_with,
        tooltip::EditorTooltip,
    },
    material_graph::menu_actions::queue_material_graph_menu_action_activation,
};
use aestra_compiler::MaterialCompiler;
use aestra_core::material::MaterialProgram;
use bevy::ui_widgets::Activate;

fn graph() -> MaterialGraphProjection {
    let p = MaterialProgram::from_ron(crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE).unwrap();
    MaterialCompiler.project_graph(&p, MaterialCompiler.compile(&p).ok().as_ref())
}
fn compose(
    projection: Option<&MaterialGraphProjection>,
    standalone: bool,
    previews: MaterialGraphPreviewState,
    locale: &str,
) -> (App, Entity) {
    let mut app = app_with(|a| {
        a.add_plugins(SvgPlugin)
            .add_observer(queue_material_graph_menu_action_activation);
    });
    let server = app.world().resource::<AssetServer>().clone();
    let localizer = Localizer::new(locale).unwrap();
    let mut root = Entity::PLACEHOLDER;
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| {
            root = parent
                .spawn_empty()
                .with_children(|p| {
                    spawn_header(
                        p,
                        projection.map(|g| ("Graph name", g)),
                        "test-frame-key",
                        Some(EditorViewId(7)),
                        &previews,
                        standalone,
                        &localizer,
                        &server,
                    )
                })
                .id();
        });
    app.world_mut().flush();
    app.update();
    (app, root)
}
fn actions(app: &mut App) -> Vec<(Entity, MaterialGraphToolbarAction)> {
    app.world_mut()
        .query::<(Entity, &MaterialGraphToolbarAction)>()
        .iter(app.world())
        .map(|(e, a)| (e, *a))
        .collect()
}
#[test]
fn composed_header_keeps_chrome_dimensions_and_standard_button_geometry() {
    let g = graph();
    let (mut app, root) = compose(
        Some(&g),
        false,
        MaterialGraphPreviewState::default(),
        "en-US",
    );
    let header = app.world().get::<Children>(root).unwrap()[0];
    let n = app.world().get::<Node>(header).unwrap();
    assert_eq!(n.height, Val::Px(38.0));
    assert_eq!(n.min_height, Val::Px(38.0));
    assert_eq!(
        app.world().get::<BackgroundColor>(header).unwrap().0,
        theme::PANEL_LIGHT
    );
    let buttons = app
        .world_mut()
        .query_filtered::<(Entity, &Node, &AccessibleLabel), With<FeathersActionButton>>()
        .iter(app.world())
        .map(|(e, n, l)| (e, n.clone(), l.0.clone()))
        .collect::<Vec<_>>();
    assert_eq!(buttons.len(), 8);
    for (e, n, label) in buttons {
        assert_eq!(n.width, Val::Px(28.0));
        assert_eq!(n.height, Val::Px(24.0));
        assert!(!label.is_empty());
        assert!(app.world().get::<EditorTooltip>(e).is_some());
    }
    assert_eq!(
        app.world_mut().query::<&UiSvg>().iter(app.world()).count(),
        8
    );
}
#[test]
fn header_actions_keep_program_scope_arrangement_and_frame_key() {
    let g = graph();
    let (app, root) = compose(
        Some(&g),
        false,
        MaterialGraphPreviewState::default(),
        "en-US",
    );
    let header = app.world().get::<Children>(root).unwrap()[0];
    let children = app.world().get::<Children>(header).unwrap();
    // Child order is the visual contract; ECS table iteration order is not.
    let a = children
        .iter()
        .filter_map(|entity| {
            app.world()
                .get::<MaterialGraphToolbarAction>(entity)
                .copied()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        a,
        vec![
            MaterialGraphToolbarAction::AddNode(g.program, Some(EditorViewId(7))),
            MaterialGraphToolbarAction::Arrange(
                g.program,
                Some(EditorViewId(7)),
                arrange::ArrangeScope::Graph
            ),
            MaterialGraphToolbarAction::ToggleAllPreviews(g.program, Some(EditorViewId(7))),
            MaterialGraphToolbarAction::LocateSource(g.program)
        ]
    );
    let frames = children
        .iter()
        .filter_map(|entity| app.world().get::<GraphFrameAction>(entity))
        .map(|a| (a.key_target().0.to_owned(), a.key_target().1))
        .collect::<Vec<_>>();
    assert_eq!(
        frames,
        vec![
            ("test-frame-key".into(), GraphFrameTarget::All),
            ("test-frame-key".into(), GraphFrameTarget::Selection)
        ]
    );
}
#[test]
fn standalone_header_has_back_action_and_shared_document_summary() {
    let g = graph();
    let (mut app, _) = compose(
        Some(&g),
        true,
        MaterialGraphPreviewState::default(),
        "en-US",
    );
    assert!(
        actions(&mut app)
            .iter()
            .any(|(_, a)| *a == MaterialGraphToolbarAction::EffectContext)
    );
    let texts = app
        .world_mut()
        .query::<&Text>()
        .iter(app.world())
        .map(|t| t.0.clone())
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|t| t.contains("Graph name")
        && t.contains("NODES")
        && t.contains("LINKS")
        && t.contains(" · ")));
}
#[test]
fn missing_projection_has_only_the_optional_back_button() {
    for standalone in [false, true] {
        let (mut app, _) = compose(
            None,
            standalone,
            MaterialGraphPreviewState::default(),
            "en-US",
        );
        assert_eq!(actions(&mut app).len(), usize::from(standalone));
        assert_eq!(
            app.world_mut()
                .query::<&GraphFrameAction>()
                .iter(app.world())
                .count(),
            0
        );
    }
}
#[test]
fn preview_visibility_selects_show_or_hide_label_in_both_locales() {
    let g = graph();
    for locale in ["en-US", "fr-FR"] {
        for visible in [false, true] {
            let mut p = MaterialGraphPreviewState::default();
            p.set_visible(g.program, graph_preview_targets(&g), visible);
            let (mut app, _) = compose(Some(&g), false, p, locale);
            let e = actions(&mut app)
                .into_iter()
                .find(|(_, a)| matches!(a, MaterialGraphToolbarAction::ToggleAllPreviews(..)))
                .unwrap()
                .0;
            assert_eq!(
                app.world().get::<AccessibleLabel>(e).unwrap().0,
                Localizer::new(locale).unwrap().text(if visible {
                    "material-graph-hide-all-previews"
                } else {
                    "material-graph-show-all-previews"
                })
            );
        }
    }
}
#[test]
fn actual_composed_material_button_activate_uses_the_shared_action_classification() {
    let g = graph();
    let (mut app, _) = compose(
        Some(&g),
        false,
        MaterialGraphPreviewState::default(),
        "en-US",
    );
    let e = actions(&mut app)[0].0;
    assert!(app.world().get::<PendingFeathersActivation>(e).is_none());
    app.world_mut().trigger(Activate { entity: e });
    app.world_mut().flush();
    assert!(app.world().get::<PendingFeathersActivation>(e).is_some());
}
#[test]
fn viewport_keys_are_distinct_for_split_views_and_the_tool_panel() {
    let p = MaterialProgramId::new();
    let a = material_graph_viewport_key(p, Some(EditorViewId(7)));
    let b = material_graph_viewport_key(p, Some(EditorViewId(88)));
    let c = material_graph_viewport_key(p, None);
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(b, c);
    assert_eq!(a, material_graph_viewport_key(p, Some(EditorViewId(7))));
}
