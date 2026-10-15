//! Production referenced headers/bars, catalog projections and attached observers on both graphs.
use super::super::{
    action_controls, execution, presentation, projections::referenced_track_projections,
    timeline_pointer::tests as pointer,
};
use super::*;
use crate::feathers::action_activation::FeathersActionButton;
use crate::feathers::list_row::KeyboardNavigableList;
use crate::{
    CurvesState, EditorSession, ProjectEffectCatalog, menus::MenuState, persistence::DocumentAction,
};
use aestra_authoring::SemanticTarget;
use aestra_core::{EffectAsset, EffectClipId};
use bevy::{
    asset::AssetPlugin,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{Activate, ListBox, ValueChange},
};

#[derive(Resource, Default)]
struct Intents(Vec<DocumentAction>);
#[derive(Resource, Default)]
struct ParentPresses(usize);
#[derive(Resource)]
struct Built {
    headers: Entity,
    tracks: Entity,
    projections: Vec<ReferencedTrackProjection>,
}

fn spawn_tracks(
    mut commands: Commands,
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    assets: Res<AssetServer>,
) {
    let clip = &session.effect.effect_clips[0];
    let projections = referenced_track_projections(&catalog, &state, clip);
    let headers = commands
        .spawn((Node::default(), KeyboardNavigableList, ListBox))
        .with_children(|parent| {
            for (index, projection) in projections.iter().enumerate() {
                match &projection.kind {
                    ReferencedTrackKind::Emitter { emitter } => {
                        spawn_referenced_emitter_track_header(
                            parent,
                            &state,
                            &localizer,
                            &projection.path,
                            projection.depth,
                            emitter,
                            (index + 1) as i16,
                        )
                    }
                    ReferencedTrackKind::EffectClip {
                        clip,
                        source_name,
                        child_count,
                    } => spawn_referenced_effect_clip_track_header(
                        parent,
                        &state,
                        &localizer,
                        &projection.path,
                        projection.depth,
                        clip,
                        source_name,
                        *child_count,
                        (index + 1) as i16,
                        &assets,
                    ),
                }
            }
        })
        .id();
    let muted = state.muted_effect_clips.contains(&clip.id);
    let suppressed = state.solo_effect_clip.is_some_and(|id| id != clip.id);
    let tracks = commands
        .spawn((Node::default(), TimelineCanvas))
        .observe(
            |_: On<super::super::timeline_pointer::PointerPress>,
             mut presses: ResMut<ParentPresses>| presses.0 += 1,
        )
        .with_children(|parent| {
            for (index, projection) in projections.iter().enumerate() {
                spawn_referenced_track_row(
                    parent,
                    &state,
                    &localizer,
                    clip,
                    projection,
                    muted,
                    suppressed,
                    (index + 1) as i16,
                );
            }
        })
        .id();
    commands.insert_resource(Built {
        headers,
        tracks,
        projections,
    });
}

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    window: Entity,
    before: EffectAsset,
    leaf: EffectAsset,
    first: EffectClipPath,
    second: EffectClipPath,
    bytes: Vec<(std::path::PathBuf, Vec<u8>)>,
}
impl Fixture {
    fn new() -> Self {
        Self::configured("en-US", false, |_, _, _| {})
    }
    fn configured(
        locale: &str,
        empty: bool,
        prepare: impl FnOnce(&mut App, &EffectClipPath, &EffectClipPath),
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut leaf = EffectAsset::new("Leaf", 4.0);
        if !empty {
            let mut emitter = Emitter::basic_sprite("Leaf emitter", 3.0);
            emitter.start_time = 0.25;
            emitter.enabled = false;
            emitter.display_color = Some([0.2, 0.4, 0.6, 0.8]);
            leaf.emitters.push(emitter);
        }
        let mut source = EffectAsset::new("Source", 4.0);
        source
            .emitters
            .push(Emitter::basic_sprite("Source emitter", 4.0));
        let mut first = EffectClip::new(leaf.id, 0.25, 1.5);
        first.source_offset = 0.2;
        let first_id = first.id;
        let second = EffectClip::new(leaf.id, 2.0, 1.0);
        let second_id = second.id;
        source.effect_clips.extend([first, second]);
        let mut bytes = Vec::new();
        for (name, effect) in [("leaf.aestra.ron", &leaf), ("source.aestra.ron", &source)] {
            let path = root.path().join(name);
            effect.save_ron(&path).unwrap();
            bytes.push((path.clone(), std::fs::read(path).unwrap()));
        }
        let mut effect = EffectAsset::new("Parent", 8.0);
        let mut clip = EffectClip::new(source.id, 1.0, 2.0);
        clip.source_offset = 0.5;
        let path = EffectClipPath::root_path(clip.id);
        let first = path.child(first_id);
        let second = path.child(second_id);
        effect.effect_clips.push(clip);
        let mut state = TimelineState::framed(8.0);
        state
            .expanded_effect_clips
            .extend([path, first.clone(), second.clone()]);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<crate::feathers::icon::SvgFile>()
        .insert_resource(EditorSession::from_test_effect(effect))
        .insert_resource(state)
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .insert_resource(Localizer::new(locale).unwrap())
        .init_resource::<CurvesState>()
        .init_resource::<MenuState>()
        .init_resource::<Intents>()
        .init_resource::<ParentPresses>()
        .add_observer(action_controls::queue_choreography_action_activation)
        .add_observer(action_controls::activate_timeline_track_entry)
        .add_observer(execution::execute_choreography_action)
        .add_observer(|event: On<DocumentAction>, mut intents: ResMut<Intents>| {
            intents.0.push(*event)
        })
        .add_systems(Startup, spawn_tracks)
        .add_systems(
            Update,
            (
                action_controls::handle_choreography_action_buttons,
                action_controls::audit_timeline_controls,
            )
                .chain(),
        );
        let window = app.world_mut().spawn(Window::default()).id();
        prepare(&mut app, &first, &second);
        let before = app.world().resource::<EditorSession>().effect.clone();
        app.update();
        Self {
            _root: root,
            app,
            window,
            before,
            leaf,
            first,
            second,
            bytes,
        }
    }
    fn clean(&self) {
        let session = self.app.world().resource::<EditorSession>();
        assert_eq!(session.effect, self.before);
        assert!(!session.dirty);
        assert!(!session.can_undo());
        for (path, bytes) in &self.bytes {
            assert_eq!(&std::fs::read(path).unwrap(), bytes);
        }
    }
    fn action<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn target(&mut self, path: &EffectClipPath, emitter: bool, header: bool) -> Entity {
        let world = self.app.world_mut();
        let mut query = world.query::<(
            Entity,
            &ChoreographyAction,
            Has<ReferencedEmitterTrackHeader>,
            Has<TimelineReferencedEmitterControl>,
        )>();
        query
            .iter(world)
            .find_map(|(entity, action, is_header, is_control)| {
                let matching = match action {
                    ChoreographyAction::SelectEffectClipEmitter { path: found, .. } => {
                        emitter && found == path
                    }
                    ChoreographyAction::SelectReferencedEffectClip(found) => {
                        !emitter && found == path
                    }
                    _ => false,
                };
                (matching && if header { is_header } else { is_control }).then_some(entity)
            })
            .unwrap()
    }
    fn leaf_target(&mut self, header: bool) -> Entity {
        let path = self.first.clone();
        self.target(&path, true, header)
    }
    fn bar(&mut self) -> Entity {
        let control = self.leaf_target(false);
        self.app.world().get::<ChildOf>(control).unwrap().parent()
    }
    fn disclosure(&mut self) -> Entity {
        let path = self.first.clone();
        let world = self.app.world_mut();
        world
            .query::<(Entity, &ChoreographyAction)>()
            .iter(world)
            .find_map(|(entity, action)| {
                matches!(action,ChoreographyAction::ToggleEffectClipExpanded(found) if *found==path)
                    .then_some(entity)
            })
            .unwrap()
    }
}
fn approx(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}
fn percent(value: Val, expected: f32) {
    let Val::Percent(actual) = value else {
        panic!("Expected percent")
    };
    approx(actual, expected);
}

#[test]
fn real_projections_build_matching_header_and_track_grid_rows() {
    let mut f = Fixture::new();
    let built = f.app.world().resource::<Built>();
    let (header_root, track_root, count) = (built.headers, built.tracks, built.projections.len());
    assert_eq!(count, 5);
    for root in [header_root, track_root] {
        let children = f.app.world().get::<Children>(root).unwrap();
        assert_eq!(children.len(), count);
        for (index, entity) in children.iter().enumerate() {
            let node = f.app.world().get::<Node>(entity).unwrap();
            assert_eq!(node.height, Val::Px(27.0));
            assert_eq!(node.grid_row, GridPlacement::start((index + 1) as i16));
        }
    }
    let world = f.app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<TimelineReferencedEmitterControl>>()
            .iter(world)
            .count(),
        count
    );
    f.clean();
}

#[test]
fn referenced_emitter_header_preserves_native_list_and_accessibility_identity() {
    let mut f = Fixture::new();
    let header = f.leaf_target(true);
    assert!(f.app.world().get::<Button>(header).is_some());
    assert!(f.app.world().get::<EditorNativeControl>(header).is_some());
    assert!(
        f.app
            .world()
            .get::<KeyboardNavigableListRow>(header)
            .is_some()
    );
    assert!(f.app.world().get::<ListItem>(header).is_some());
    assert!(
        f.app
            .world()
            .get::<AccessibleLabel>(header)
            .unwrap()
            .0
            .contains("Leaf emitter")
    );
    assert!(f.app.world().get::<FeathersActionButton>(header).is_none());
    f.clean();
}

#[test]
fn selected_emitter_header_and_bar_keep_selected_path_styling() {
    let mut f = Fixture::configured("en-US", false, |app, first, _| {
        let emitter = app
            .world()
            .resource::<ProjectEffectCatalog>()
            .entries()
            .iter()
            .find(|entry| entry.display_name == "Leaf")
            .unwrap()
            .reference
            .unwrap();
        let emitter = app
            .world()
            .resource::<ProjectEffectCatalog>()
            .cached_effect(emitter)
            .unwrap()
            .emitters[0]
            .id;
        app.world_mut()
            .resource_mut::<TimelineState>()
            .inspected_child = Some(EffectClipChildSelection::Emitter {
            path: first.clone(),
            emitter,
        });
    });
    let header = f.leaf_target(true);
    let bar = f.bar();
    assert!(f.app.world().get::<Selected>(header).is_some());
    assert_eq!(
        f.app.world().get::<BackgroundColor>(header).unwrap().0,
        crate::theme::SELECTION.with_alpha(0.75)
    );
    assert_eq!(
        f.app.world().get::<Node>(bar).unwrap().border,
        UiRect::all(Val::Px(2.0))
    );
    assert_eq!(
        f.app.world().get::<BorderColor>(bar).unwrap().top,
        crate::theme::TEXT
    );
    f.clean();
}

#[test]
fn selected_nested_clip_header_has_its_own_path_and_selected_marker() {
    let mut f = Fixture::configured("en-US", false, |app, first, _| {
        app.world_mut()
            .resource_mut::<TimelineState>()
            .inspected_child = Some(EffectClipChildSelection::EffectClip {
            path: first.clone(),
        });
    });
    let path = f.first.clone();
    let header = f.target(&path, false, true);
    assert!(f.app.world().get::<Selected>(header).is_some());
    let control = f.target(&path, false, false);
    let bar = f.app.world().get::<ChildOf>(control).unwrap().parent();
    assert_eq!(
        f.app.world().get::<Node>(bar).unwrap().border,
        UiRect::all(Val::Px(2.0))
    );
    f.clean();
}

#[test]
fn nested_header_indentation_tracks_projection_depth() {
    let mut f = Fixture::new();
    let leaf = f.leaf_target(true);
    let path = f.first.clone();
    let clip = f.target(&path, false, true);
    assert_eq!(
        f.app.world().get::<Node>(leaf).unwrap().padding.left,
        Val::Px(51.0)
    );
    assert_eq!(
        f.app.world().get::<Node>(clip).unwrap().padding.left,
        Val::Px(29.0)
    );
    f.clean();
}

#[test]
fn disabled_referenced_emitter_name_and_custom_bar_color_are_preserved() {
    let mut f = Fixture::new();
    let header = f.leaf_target(true);
    let world = f.app.world_mut();
    let name = world
        .query::<(Entity, &Text, &TextColor, &ChildOf)>()
        .iter(world)
        .find(|(_, text, _, parent)| parent.parent() == header && text.0 == "Leaf emitter")
        .unwrap();
    assert_eq!(name.2.0, crate::theme::TEXT_FAINT);
    let bar = f.bar();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(bar).unwrap().0,
        Color::srgba(0.2, 0.4, 0.6, 0.20)
    );
    f.clean();
}

#[test]
fn muted_and_solo_suppressed_children_use_dim_bar_fill() {
    for muted in [true, false] {
        let mut f = Fixture::configured("en-US", false, |app, _, _| {
            let id = app.world().resource::<EditorSession>().effect.effect_clips[0].id;
            let mut state = app.world_mut().resource_mut::<TimelineState>();
            if muted {
                state.muted_effect_clips.insert(id);
            } else {
                state.solo_effect_clip = Some(EffectClipId::new());
            }
        });
        let bar = f.bar();
        assert_eq!(
            f.app.world().get::<BackgroundColor>(bar).unwrap().0,
            Color::srgba(0.2, 0.4, 0.6, 0.08)
        );
        f.clean();
    }
}

#[test]
fn trimmed_ancestor_timing_maps_real_child_geometry_and_identity() {
    let mut f = Fixture::new();
    let bar = f.bar();
    let node = f.app.world().get::<Node>(bar).unwrap();
    percent(node.left, 12.5);
    percent(node.width, 15.625);
    let marker = f.app.world().get::<TimelineReferencedEmitter>(bar).unwrap();
    assert_eq!(marker.clip, f.before.effect_clips[0].id);
    approx(marker.source_start, 0.5);
    approx(marker.source_duration, 1.25);
    f.clean();
}

#[test]
fn no_timing_children_keep_empty_rows_and_headers_in_layout() {
    let f = Fixture::configured("en-US", false, |app, _, _| {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        let clip = &mut session.effect.effect_clips[0];
        clip.source_offset = 3.8;
        clip.duration = 0.1;
    });
    let built = f.app.world().resource::<Built>();
    assert!(
        built
            .projections
            .iter()
            .any(|projection| projection.timing.is_none())
    );
    for (row, projection) in f
        .app
        .world()
        .get::<Children>(built.tracks)
        .unwrap()
        .iter()
        .zip(&built.projections)
    {
        if projection.timing.is_none() {
            assert!(f.app.world().get::<Children>(row).is_none());
        }
    }
    assert_eq!(
        f.app.world().get::<Children>(built.headers).unwrap().len(),
        built.projections.len()
    );
    f.clean();
}

#[test]
fn offscreen_reference_bars_hide_without_dropping_controls_or_rows() {
    let mut f = Fixture::configured("en-US", false, |app, _, _| {
        app.world_mut().resource_mut::<TimelineState>().view = super::super::state::TimelineView {
            start: 6.0,
            end: 8.0,
        };
    });
    let bar = f.bar();
    assert_eq!(
        f.app.world().get::<Node>(bar).unwrap().display,
        Display::None
    );
    assert!(
        f.app
            .world()
            .get::<Children>(bar)
            .unwrap()
            .iter()
            .any(|child| f
                .app
                .world()
                .get::<TimelineReferencedEmitterControl>(child)
                .is_some())
    );
    f.clean();
}

#[test]
fn reused_sources_keep_independent_branch_selection_actions() {
    let mut f = Fixture::new();
    let first = f.leaf_target(false);
    let second_path = f.second.clone();
    let second = f.target(&second_path, true, false);
    assert_ne!(first, second);
    f.action(pointer::click(second, f.window, PointerButton::Primary, 1));
    assert_eq!(
        f.app.world().resource::<TimelineState>().inspected_child,
        Some(EffectClipChildSelection::Emitter {
            path: f.second.clone(),
            emitter: f.leaf.emitters[0].id
        })
    );
    f.clean();
}

#[test]
fn bar_click_selects_root_clip_and_clears_curve_selection_without_edits() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<CurvesState>()
        .select_key_channel(aestra_core::ModuleId::new(), 0, 0, None);
    let control = f.leaf_target(false);
    f.action(pointer::click(control, f.window, PointerButton::Primary, 1));
    assert_eq!(
        f.app.world().resource::<EditorSession>().selection.primary,
        SemanticTarget::EffectClip(f.before.effect_clips[0].id)
    );
    assert!(!f.app.world().resource::<CurvesState>().has_selection());
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.clean();
}

#[test]
fn secondary_bar_click_does_not_select_or_request_source_open() {
    let mut f = Fixture::new();
    let before = f.app.world().resource::<EditorSession>().selection.primary;
    let control = f.leaf_target(false);
    f.action(pointer::click(
        control,
        f.window,
        PointerButton::Secondary,
        2,
    ));
    assert_eq!(
        f.app.world().resource::<EditorSession>().selection.primary,
        before
    );
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .inspected_child
            .is_none()
    );
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.clean();
}

#[test]
fn bar_double_click_requests_leaf_emitter_source_without_replacing_document() {
    let mut f = Fixture::new();
    let control = f.leaf_target(false);
    f.action(pointer::click(control, f.window, PointerButton::Primary, 2));
    assert_eq!(
        f.app.world().resource::<Intents>().0,
        vec![DocumentAction::OpenSourceEmitter(
            f.leaf.id.into(),
            f.leaf.emitters[0].id
        )]
    );
    f.clean();
}

#[test]
fn header_double_click_and_list_activation_use_actual_constructed_row() {
    let mut f = Fixture::new();
    let header = f.leaf_target(true);
    let list = f.app.world().resource::<Built>().headers;
    f.action(ValueChange {
        source: list,
        value: header,
        is_final: true,
    });
    assert_eq!(
        f.app.world().resource::<TimelineState>().inspected_child,
        Some(EffectClipChildSelection::Emitter {
            path: f.first.clone(),
            emitter: f.leaf.emitters[0].id
        })
    );
    f.action(pointer::click(header, f.window, PointerButton::Primary, 2));
    assert_eq!(
        f.app.world().resource::<Intents>().0,
        vec![DocumentAction::OpenSourceEmitter(
            f.leaf.id.into(),
            f.leaf.emitters[0].id
        )]
    );
    f.clean();
}

#[test]
fn disclosure_is_real_tool_button_and_toggles_only_nested_expansion() {
    let mut f = Fixture::new();
    let disclosure = f.disclosure();
    assert!(
        f.app
            .world()
            .get::<FeathersActionButton>(disclosure)
            .is_some()
    );
    assert!(
        f.app
            .world()
            .get::<EditorNativeControl>(disclosure)
            .is_none()
    );
    let first = f.first.clone();
    let root = EffectClipPath::root_path(f.before.effect_clips[0].id);
    f.action(Activate { entity: disclosure });
    f.app.update();
    let state = f.app.world().resource::<TimelineState>();
    assert!(!state.expanded_effect_clips.contains(&first));
    assert!(state.expanded_effect_clips.contains(&root));
    assert!(state.expanded_effect_clips.contains(&f.second));
    f.clean();
}

#[test]
fn empty_source_disclosure_is_hidden_and_expanded_source_uses_down_icon() {
    let mut empty = Fixture::configured("en-US", true, |_, _, _| {});
    let disclosure = empty.disclosure();
    assert_eq!(
        empty.app.world().get::<Node>(disclosure).unwrap().display,
        Display::None
    );
    empty.clean();
    let mut f = Fixture::new();
    let disclosure = f.disclosure();
    let icon = f
        .app
        .world()
        .get::<Children>(disclosure)
        .unwrap()
        .iter()
        .find_map(|child| f.app.world().get::<UiSvg>(child))
        .unwrap();
    assert_eq!(
        f.app
            .world()
            .resource::<AssetServer>()
            .get_path(icon.0.id())
            .unwrap()
            .path(),
        std::path::Path::new("icons/chevron-down.svg")
    );
    f.clean();
}

#[test]
fn read_only_badges_and_accessible_labels_follow_both_locales() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::configured(locale, false, |_, _, _| {});
        let header = f.leaf_target(true);
        let expected = f
            .app
            .world()
            .resource::<Localizer>()
            .text("timeline-read-only-short");
        assert!(
            f.app
                .world()
                .get::<Children>(header)
                .unwrap()
                .iter()
                .any(|child| f
                    .app
                    .world()
                    .get::<Text>(child)
                    .is_some_and(|text| text.0 == expected))
        );
        assert!(
            f.app
                .world()
                .get::<AccessibleLabel>(header)
                .unwrap()
                .0
                .contains("Leaf emitter")
        );
        f.clean();
    }
}

#[test]
fn bar_press_is_stopped_by_attached_control_observer() {
    let mut f = Fixture::new();
    let control = f.leaf_target(false);
    f.action(pointer::press(
        control,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert_eq!(f.app.world().resource::<ParentPresses>().0, 0);
    f.clean();
}

#[test]
fn constructed_reference_geometry_remains_compatible_with_live_root_trim() {
    let mut f = Fixture::new();
    let bar = f.bar();
    let clip = f.before.effect_clips[0].clone();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .effect_clip_drag = Some(EffectClipTimelineDrag {
        clip: clip.id,
        kind: TimelineDragKind::TrimStart,
        pointer_start: 0.0,
        original_start: clip.start_time,
        original_source_offset: clip.source_offset,
        original_duration: clip.duration,
        current_start: 2.0,
        current_source_offset: 1.0,
        current_duration: 1.0,
        source_duration: 4.0,
        source_looping: false,
    });
    f.app
        .add_systems(Update, presentation::update_effect_clip_visuals);
    f.app.update();
    let node = f.app.world().get::<Node>(bar).unwrap();
    percent(node.left, 25.0);
    percent(node.width, 9.375);
    f.clean();
}
