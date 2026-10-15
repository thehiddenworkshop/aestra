//! Real lane construction, raster requests, activation and pointer consumers on both engines.
use super::super::{
    action_controls, automation::emitter_automation_lanes, automation_data::automation_lane_keys,
    execution, navigation_cursor::OverrideCursor, timeline_pointer::tests as pointer,
};
use super::*;
use crate::{
    EditorModuleRegistry, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    menus::MenuState,
};
use aestra_core::{
    ColorKey, Curve, CurveKey, EffectAsset, EffectParameter, EmitterRegion, ModuleParameters,
    ParameterId, PropertyEvaluationDomain, PropertySource, PropertySourceValue, Value, Vec3Curve,
};
use bevy::{
    asset::AssetPlugin,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::Activate,
    window::{CursorIcon, PrimaryWindow},
};

#[derive(Clone, Copy)]
enum Flavor {
    Scalar,
    Gradient,
    Vector,
    Bound,
    Bank,
}
#[derive(Resource)]
struct LaneRequest(AutomationLaneProjection);
#[derive(Resource)]
struct Roots {
    header: Entity,
    canvas: Entity,
}
#[derive(Resource, Default)]
struct ParentInput {
    clicks: usize,
    presses: usize,
    starts: usize,
}
#[derive(Resource, Default)]
struct DispatchedActions {
    adds: usize,
    deletes: usize,
}

fn curve() -> Curve {
    Curve::new(vec![
        CurveKey::new(0.0, 1.0),
        CurveKey::new(0.5, 2.0),
        CurveKey::new(1.0, 3.0),
    ])
}

fn spawn_lanes(
    mut commands: Commands,
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    curves: Res<CurvesState>,
    localizer: Res<Localizer>,
    assets: Res<AssetServer>,
    request: Res<LaneRequest>,
) {
    let header = commands
        .spawn(Node::default())
        .with_children(|parent| {
            spawn_automation_lane_header(parent, &localizer, &state, &request.0, 4, &assets);
        })
        .id();
    let canvas = commands
        .spawn((
            TimelineCanvas,
            Node::default(),
            ComputedNode {
                size: Vec2::new(400.0, 200.0),
                ..default()
            },
        ))
        .observe(
            |_: On<super::super::timeline_pointer::PointerClick>,
             mut input: ResMut<ParentInput>| input.clicks += 1,
        )
        .observe(
            |_: On<super::super::timeline_pointer::PointerPress>,
             mut input: ResMut<ParentInput>| input.presses += 1,
        )
        .observe(
            |_: On<super::super::timeline_pointer::PointerDragStart>,
             mut input: ResMut<ParentInput>| input.starts += 1,
        )
        .with_children(|parent| {
            spawn_automation_lane_row(
                parent,
                &session,
                &state,
                &curves,
                &localizer,
                &session.effect.emitters[0],
                &request.0,
                4,
            );
        })
        .id();
    commands.insert_resource(Roots { header, canvas });
}

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    window: Entity,
    lane: AutomationLaneId,
    before: EffectAsset,
}
impl Fixture {
    fn new(flavor: Flavor, selected: bool, locale: &str) -> Self {
        Self::configured(flavor, selected, locale, |_, _| {})
    }
    fn configured(
        flavor: Flavor,
        selected: bool,
        locale: &str,
        prepare: impl FnOnce(&mut App, &AutomationLaneId),
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = EffectAsset::new("Lanes", 8.0);
        let mut emitter = Emitter::basic_sprite("Emitter", 4.0);
        emitter.start_time = 1.0;
        let appearance = emitter
            .modules
            .iter_mut()
            .find(|m| matches!(m.parameters, ModuleParameters::Appearance { .. }))
            .unwrap();
        let ModuleParameters::Appearance { size, color, .. } = &mut appearance.parameters else {
            unreachable!()
        };
        *size = curve();
        color.keys = vec![
            ColorKey::new(0.0, [1.0, 0.0, 0.0, 1.0]),
            ColorKey::new(0.5, [0.0, 1.0, 0.0, 1.0]),
            ColorKey::new(1.0, [0.0, 0.0, 1.0, 1.0]),
        ];
        let source = PropertySource::Curve(PropertyEvaluationDomain::ParticleLife);
        match flavor {
            Flavor::Bound => {
                let id = ParameterId::new();
                appearance.bindings.insert("size".into(), id);
                effect.parameters.push(EffectParameter {
                    id,
                    name: "Public size".into(),
                    exposed: true,
                    default: Value::Curve(Curve::new(vec![
                        CurveKey::new(0.0, 4.0),
                        CurveKey::new(0.5, 5.0),
                        CurveKey::new(1.0, 6.0),
                    ])),
                });
            }
            Flavor::Bank => {
                appearance.property_sources.insert("size".into(), source);
                appearance.property_source_values.insert(
                    "size".into(),
                    vec![PropertySourceValue::new(source, Value::Curve(curve()))],
                );
            }
            Flavor::Vector => {
                let motion = emitter
                    .modules
                    .iter_mut()
                    .find(|m| m.module_type.0 == aestra_core::MODULE_MOTION)
                    .unwrap();
                motion.property_sources.insert("gravity".into(), source);
                motion.property_source_values.insert(
                    "gravity".into(),
                    vec![PropertySourceValue::new(
                        source,
                        Value::Vec3Curve(Vec3Curve {
                            curves: std::array::from_fn(|_| curve()),
                        }),
                    )],
                );
            }
            _ => {}
        }
        effect.emitters.push(emitter);
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new(locale).unwrap();
        let (parameter, channel) = match flavor {
            Flavor::Gradient => ("color", None),
            Flavor::Vector => ("gravity", Some(1)),
            _ => ("size", None),
        };
        let projection =
            emitter_automation_lanes(&effect, &effect.emitters[0], &registry, &localizer)
                .into_iter()
                .find(|lane| lane.id.parameter == parameter && lane.id.channel == channel)
                .unwrap();
        let lane = projection.id.clone();
        let mut session = EditorSession::from_test_effect(effect);
        session.select_emitter(lane.emitter);
        let mut state = TimelineState::framed(8.0);
        state.snap = super::super::TimelineSnapMode::None;
        let mut curves = CurvesState::default();
        if selected {
            curves.select_key_channel(lane.module, lane.input, 1, lane.channel);
            state.selected_automation_key = Some(TimelineAutomationKeySelection {
                lane: lane.clone(),
                key: 1,
            });
        }
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<crate::feathers::icon::SvgFile>()
        .insert_resource(session)
        .insert_resource(state)
        .insert_resource(curves)
        .insert_resource(registry)
        .insert_resource(localizer)
        .insert_resource(LaneRequest(projection))
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<OverrideCursor>()
        .init_resource::<MenuState>()
        .init_resource::<ParentInput>()
        .init_resource::<DispatchedActions>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
        .init_resource::<automation_curve::AutomationCurveImageCache>()
        .add_observer(action_controls::queue_choreography_action_activation)
        .add_observer(execution::execute_choreography_action)
        .add_observer(
            |action: On<ChoreographyAction>, mut counts: ResMut<DispatchedActions>| match &*action {
                ChoreographyAction::AddAutomationKey(_) => counts.adds += 1,
                ChoreographyAction::DeleteAutomationKey(_) => counts.deletes += 1,
                _ => {}
            },
        )
        .add_observer(
            |action: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                routing.execute(*action, &mut commands, |_, _, _, _| {
                    panic!("No filesystem history expected")
                });
            },
        )
        .add_systems(Startup, spawn_lanes)
        .add_systems(
            Update,
            (
                action_controls::handle_choreography_action_buttons,
                action_controls::audit_timeline_controls,
                automation_curve::rasterize_automation_curves,
            )
                .chain(),
        );
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow, CursorIcon::default()))
            .id();
        prepare(&mut app, &lane);
        let before = app.world().resource::<EditorSession>().effect.clone();
        app.update();
        Self {
            _root: root,
            app,
            window,
            lane,
            before,
        }
    }
    fn keys(&mut self) -> Vec<Entity> {
        let world = self.app.world_mut();
        let mut query=world.query_filtered::<(Entity,&TimelineAutomationKeySelection),With<TimelineAutomationKey>>();
        let mut keys: Vec<_> = query
            .iter(world)
            .map(|(entity, selection)| (selection.key, entity))
            .collect();
        keys.sort_by_key(|k| k.0);
        keys.into_iter().map(|(_, entity)| entity).collect()
    }
    fn graph(&mut self) -> Entity {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<TimelineAutomationLaneGraph>>()
            .single(world)
            .unwrap()
    }
    fn button(&mut self, remove: bool) -> Option<Entity> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &ChoreographyAction)>()
            .iter(world)
            .find_map(|(entity, action)| {
                (if remove {
                    matches!(action, ChoreographyAction::DeleteAutomationKey(_))
                } else {
                    matches!(action, ChoreographyAction::AddAutomationKey(_))
                })
                .then_some(entity)
            })
    }
    fn action<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn begin(&mut self, entity: Entity) {
        self.action(pointer::start(
            entity,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn end(&mut self, entity: Entity) {
        self.action(pointer::end(
            entity,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn clean(&self) {
        let session = self.app.world().resource::<EditorSession>();
        assert_eq!(session.effect, self.before);
        assert!(!session.dirty);
        assert!(!session.can_undo());
    }
    fn roundtrip(&mut self) {
        let after = self.app.world().resource::<EditorSession>().effect.clone();
        assert_ne!(after, self.before);
        self.action(HistoryAction::Undo);
        assert_eq!(
            self.app.world().resource::<EditorSession>().effect,
            self.before
        );
        self.action(HistoryAction::Redo);
        assert_eq!(self.app.world().resource::<EditorSession>().effect, after);
    }
}
fn approx(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}
fn percent(value: Val, expected: f32) {
    let Val::Percent(actual) = value else {
        panic!("Expected percent, got {value:?}")
    };
    approx(actual, expected);
}

#[test]
fn production_metadata_uses_real_registry_input_identity_and_unique_lanes() {
    let f = Fixture::new(Flavor::Scalar, false, "en-US");
    let session = f.app.world().resource::<EditorSession>();
    let registry = f.app.world().resource::<EditorModuleRegistry>();
    let localizer = f.app.world().resource::<Localizer>();
    let lanes = emitter_automation_lanes(
        &session.effect,
        &session.effect.emitters[0],
        registry,
        localizer,
    );
    let ids: std::collections::BTreeSet<_> = lanes.iter().map(|lane| lane.id.clone()).collect();
    assert_eq!(ids.len(), lanes.len());
    for lane in &lanes {
        let module = session.effect.emitters[0]
            .modules
            .iter()
            .find(|m| m.id == lane.id.module)
            .unwrap();
        assert_eq!(
            registry.0.get(&module.module_type).unwrap().inputs[lane.id.input as usize].name,
            lane.id.parameter
        );
    }
    f.clean();
}

#[test]
fn real_locale_labels_and_tooltips_are_used_in_both_languages() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::new(Flavor::Scalar, true, locale);
        let localizer = f.app.world().resource::<Localizer>();
        let expected = localizer.text("properties-input-size");
        assert_eq!(f.app.world().resource::<LaneRequest>().0.label, expected);
        let add = f.button(false).unwrap();
        let tip = f
            .app
            .world()
            .get::<EditorTooltip>(add)
            .unwrap()
            .accessible_label();
        assert!(tip.contains("Insert"));
        let remove = f.button(true).unwrap();
        assert!(
            f.app
                .world()
                .get::<EditorTooltip>(remove)
                .unwrap()
                .accessible_label()
                .contains("Delete")
        );
        f.clean();
    }
}

#[test]
fn constant_and_unknown_modules_do_not_create_automation_lanes() {
    let f = Fixture::new(Flavor::Scalar, false, "en-US");
    let mut effect = f.before.clone();
    for module in &mut effect.emitters[0].modules {
        for parameter in ["size", "opacity", "color"] {
            module
                .property_sources
                .insert(parameter.into(), PropertySource::Constant);
        }
    }
    let registry = f.app.world().resource::<EditorModuleRegistry>();
    let localizer = f.app.world().resource::<Localizer>();
    assert!(emitter_automation_lanes(&effect, &effect.emitters[0], registry, localizer).is_empty());
    let mut unknown = f.before.clone();
    let appearance = unknown.emitters[0]
        .modules
        .iter_mut()
        .find(|module| matches!(module.parameters, ModuleParameters::Appearance { .. }))
        .unwrap();
    appearance.module_type = aestra_core::ModuleTypeId("missing.module".into());
    assert!(
        emitter_automation_lanes(&unknown, &unknown.emitters[0], registry, localizer).is_empty()
    );
}

#[test]
fn bound_parameter_default_is_the_rendered_lane_value() {
    let f = Fixture::new(Flavor::Bound, false, "en-US");
    let lane = &f.app.world().resource::<LaneRequest>().0;
    let AutomationLaneKeys::Curve(keys, _) = &lane.keys else {
        panic!("curve expected")
    };
    assert_eq!(keys[1].value, 5.0);
    let value = automation_lane_keys(&f.before, &lane.id).unwrap();
    assert_eq!(value.curve_value(1), Some(5.0));
    f.clean();
}

#[test]
fn vector_source_bank_projects_three_independent_channel_lanes() {
    let f = Fixture::new(Flavor::Vector, true, "en-US");
    let registry = f.app.world().resource::<EditorModuleRegistry>();
    let localizer = f.app.world().resource::<Localizer>();
    let lanes: Vec<_> =
        emitter_automation_lanes(&f.before, &f.before.emitters[0], registry, localizer)
            .into_iter()
            .filter(|lane| lane.id.parameter == "gravity")
            .collect();
    assert_eq!(lanes.len(), 3);
    for (axis, lane) in lanes.iter().enumerate() {
        assert_eq!(lane.id.channel, Some(axis as u8));
        assert!(lane.label.ends_with(["X", "Y", "Z"][axis]));
        assert_eq!(lane.keys.len(), 3);
    }
    f.clean();
}

#[test]
fn scalar_lane_has_native_key_controls_and_matching_header_geometry() {
    let mut f = Fixture::configured(Flavor::Scalar, true, "en-US", |app, lane| {
        app.world_mut()
            .resource_mut::<TimelineState>()
            .automation_lane_heights
            .insert(lane.clone(), 150.0);
    });
    let roots = f.app.world().resource::<Roots>();
    let (header, canvas) = (roots.header, roots.canvas);
    assert_ne!(header, canvas);
    let world = f.app.world_mut();
    let rows: Vec<_> = world
        .query_filtered::<(&Node, &TimelineChoreographyGridRow), With<TimelineAutomationLane>>()
        .iter(world)
        .collect();
    assert_eq!(rows.len(), 2);
    for (node, row) in rows {
        assert_eq!(node.height, Val::Px(150.0));
        assert_eq!(node.grid_row, GridPlacement::start(4));
        assert_eq!(row.0, 4);
    }
    let keys = f.keys();
    assert_eq!(keys.len(), 3);
    for (index, key) in keys.iter().enumerate() {
        assert!(f.app.world().get::<Button>(*key).is_some());
        assert!(f.app.world().get::<EditorNativeControl>(*key).is_some());
        let node = f.app.world().get::<Node>(*key).unwrap();
        assert_eq!(node.width, Val::Px(11.0));
        percent(node.left, (1.0 + index as f32 * 2.0) / 8.0 * 100.0);
    }
    f.clean();
}

#[test]
fn selected_scalar_key_uses_accent_and_unselected_keys_are_muted() {
    let mut f = Fixture::new(Flavor::Scalar, true, "en-US");
    let keys = f.keys();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(keys[1]).unwrap().0,
        theme::ACCENT
    );
    assert_eq!(
        f.app.world().get::<BackgroundColor>(keys[0]).unwrap().0,
        theme::TEXT_MUTED
    );
    f.clean();
}

#[test]
fn gradient_keys_are_full_height_handles_with_selected_line_width() {
    let mut f = Fixture::new(Flavor::Gradient, true, "en-US");
    let keys = f.keys();
    assert_eq!(keys.len(), 3);
    for (index, key) in keys.iter().enumerate() {
        let node = f.app.world().get::<Node>(*key).unwrap();
        assert_eq!(node.height, Val::Percent(100.0));
        assert_eq!(node.top, Val::Px(0.0));
        let line = f.app.world().get::<Children>(*key).unwrap()[0];
        assert_eq!(
            f.app.world().get::<Node>(line).unwrap().width,
            Val::Px(if index == 1 { 3.0 } else { 2.0 })
        );
        assert_eq!(f.app.world().get::<Pickable>(line), Some(&Pickable::IGNORE));
    }
    f.clean();
}

#[test]
fn vector_key_highlight_respects_selected_channel() {
    let mut f = Fixture::configured(Flavor::Vector, true, "en-US", |app, lane| {
        app.world_mut()
            .resource_mut::<CurvesState>()
            .select_key_channel(lane.module, lane.input, 1, Some(0));
    });
    for key in f.keys() {
        assert_eq!(
            f.app.world().get::<BackgroundColor>(key).unwrap().0,
            theme::TEXT_MUTED
        );
    }
    f.clean();
}

#[test]
fn header_has_add_always_and_delete_only_for_its_selected_lane() {
    let mut f = Fixture::new(Flavor::Scalar, false, "en-US");
    assert!(f.button(false).is_some());
    assert!(f.button(true).is_none());
    let mut selected = Fixture::new(Flavor::Scalar, true, "en-US");
    let add = selected.button(false).unwrap();
    let remove = selected.button(true).unwrap();
    assert!(
        selected
            .app
            .world()
            .get::<FeathersActionButton>(add)
            .is_some()
    );
    assert!(
        selected
            .app
            .world()
            .get::<EditorNativeControl>(add)
            .is_none()
    );
    assert_eq!(
        selected.app.world().get::<ChoreographyAction>(add).unwrap(),
        &ChoreographyAction::AddAutomationKey(selected.lane.clone())
    );
    assert!(
        matches!(selected.app.world().get::<ChoreographyAction>(remove).unwrap(),ChoreographyAction::DeleteAutomationKey(selection) if selection.lane==selected.lane && selection.key==1)
    );
}

#[test]
fn selected_region_source_offset_controls_graph_and_key_times() {
    let mut f = Fixture::configured(Flavor::Scalar, false, "en-US", |app, lane| {
        let region = EmitterRegion::new(3.0, 0.5, 1.0);
        let region_id = region.id;
        app.world_mut()
            .resource_mut::<EditorSession>()
            .effect
            .emitters[0]
            .regions = vec![region];
        app.world_mut()
            .resource_mut::<TimelineState>()
            .selected_emitter_region = Some((lane.emitter, region_id));
    });
    let graph = f.graph();
    let node = f.app.world().get::<Node>(graph).unwrap();
    percent(node.left, 31.25);
    percent(node.width, 50.0);
    let keys = f.keys();
    percent(f.app.world().get::<Node>(keys[0]).unwrap().left, 31.25);
    f.clean();
}

#[test]
fn offscreen_graph_and_keys_start_hidden_but_keep_identity() {
    let mut f = Fixture::configured(Flavor::Scalar, false, "en-US", |app, _| {
        app.world_mut().resource_mut::<TimelineState>().view = super::super::state::TimelineView {
            start: 6.0,
            end: 8.0,
        };
    });
    let graph = f.graph();
    assert_eq!(
        f.app.world().get::<Node>(graph).unwrap().display,
        Display::None
    );
    for key in f.keys() {
        assert_eq!(
            f.app.world().get::<Node>(key).unwrap().display,
            Display::None
        );
        assert!(
            f.app
                .world()
                .get::<TimelineAutomationKeySelection>(key)
                .is_some()
        );
    }
    f.clean();
}

#[test]
fn raster_composition_produces_image_and_reuses_it_while_idle() {
    let mut f = Fixture::new(Flavor::Scalar, false, "en-US");
    let graph = f.graph();
    let raster = f.app.world().get::<Children>(graph).unwrap()[0];
    let image = f
        .app
        .world()
        .get::<ImageNode>(raster)
        .unwrap()
        .image
        .clone();
    assert!(
        f.app
            .world()
            .resource::<Assets<Image>>()
            .get(&image)
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .iter()
            .any(|v| *v != 0)
    );
    assert_eq!(
        f.app.world().get::<Pickable>(raster),
        Some(&Pickable::IGNORE)
    );
    f.app.update();
    assert_eq!(f.app.world().get::<ImageNode>(raster).unwrap().image, image);
    f.clean();
}

#[test]
fn key_click_selects_through_attached_production_observer_without_editing() {
    let mut f = Fixture::new(Flavor::Scalar, false, "en-US");
    let key = f.keys()[1];
    f.action(pointer::click(key, f.window, PointerButton::Primary, 1));
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .key,
        1
    );
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .selected_automation_key
            .as_ref()
            .unwrap()
            .lane,
        f.lane
    );
    assert_eq!(f.app.world().resource::<ParentInput>().clicks, 0);
    f.clean();
}

#[test]
fn key_press_does_not_reach_canvas_seek_target() {
    let mut f = Fixture::new(Flavor::Scalar, false, "en-US");
    let key = f.keys()[1];
    f.action(pointer::press(
        key,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert_eq!(f.app.world().resource::<ParentInput>().presses, 0);
    f.clean();
}

#[test]
fn spawned_key_drag_is_preview_then_one_globally_reversible_edit() {
    let mut f = Fixture::new(Flavor::Scalar, false, "en-US");
    let key = f.keys()[1];
    f.begin(key);
    assert_eq!(f.app.world().resource::<ParentInput>().starts, 0);
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .automation_key_drag
            .is_some()
    );
    f.action(pointer::motion_delta(key, f.window, Vec2::new(50.0, 0.0)));
    f.clean();
    f.end(key);
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .automation_key_drag
            .is_none()
    );
    f.roundtrip();
}

#[test]
fn real_add_button_activation_dispatches_once_and_undo_redo_restores_keys() {
    let mut f = Fixture::new(Flavor::Scalar, false, "en-US");
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(2.0);
    let add = f.button(false).unwrap();
    f.action(Activate { entity: add });
    f.app.update();
    f.roundtrip();
    let after = f.app.world().resource::<EditorSession>().effect.clone();
    f.app.update();
    assert_eq!(f.app.world().resource::<EditorSession>().effect, after);
    assert_eq!(f.app.world().resource::<DispatchedActions>().adds, 1);
    assert!(
        f.app
            .world()
            .get::<crate::feathers::action_activation::PendingFeathersActivation>(add)
            .is_none()
    );
}

#[test]
fn real_delete_button_activation_deletes_selected_middle_key() {
    let mut f = Fixture::new(Flavor::Scalar, true, "en-US");
    let remove = f.button(true).unwrap();
    f.action(Activate { entity: remove });
    f.app.update();
    assert_eq!(f.app.world().resource::<DispatchedActions>().deletes, 1);
    assert_eq!(
        automation_lane_keys(&f.app.world().resource::<EditorSession>().effect, &f.lane)
            .unwrap()
            .len(),
        2
    );
    f.roundtrip();
}

#[test]
fn graph_double_click_adds_key_using_the_constructed_graph_observer() {
    let mut f = Fixture::new(Flavor::Bank, false, "en-US");
    let graph = f.graph();
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(graph)
        .unwrap()
        .normalized = Some(Vec2::new(-0.25, 0.0));
    f.action(pointer::click(graph, f.window, PointerButton::Primary, 2));
    assert_eq!(
        automation_lane_keys(&f.app.world().resource::<EditorSession>().effect, &f.lane)
            .unwrap()
            .len(),
        4
    );
    f.roundtrip();
}

#[test]
fn constructed_resize_handle_changes_both_rows_without_semantic_edits() {
    let mut f = Fixture::new(Flavor::Scalar, false, "en-US");
    let world = f.app.world_mut();
    let handle = world
        .query_filtered::<Entity, With<TimelineAutomationLaneResizeHandle>>()
        .single(world)
        .unwrap();
    f.begin(handle);
    f.action(pointer::motion_delta(
        handle,
        f.window,
        Vec2::new(0.0, 30.0),
    ));
    f.end(handle);
    let world = f.app.world_mut();
    for node in world
        .query_filtered::<&Node, With<TimelineAutomationLane>>()
        .iter(world)
    {
        assert_eq!(
            node.height,
            Val::Px(automation_curve::DEFAULT_HEIGHT + 30.0)
        );
    }
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .automation_lane_resize
            .is_none()
    );
    f.clean();
}
