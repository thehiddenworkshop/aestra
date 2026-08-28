//! Preview viewport ownership: rendering, navigation, grid, display modes, and gizmos.

use crate::{
    EditorAction, FeathersActionButton, MenuState,
    feathers::tooltip::EditorTooltip,
    inspector::module_parameter,
    localization::Localizer,
    profiler::{ProfilerFrameSample, ProfilerState},
    session::EditorSession,
    theme, ui_shell,
};
use aestra_authoring::{EffectCommand, EffectTransaction, SemanticTarget};
use aestra_bevy::{
    ActiveBackend, AestraSet, EffectPlayer, EffectRenderMode, EffectRuntimeStatus, EmitterId,
    EmitterShape, EmitterTransform, ModuleId, Value,
};
use aestra_runtime::CompiledEffect;
use bevy::{
    app::TransformGizmoRenderStep,
    camera::{Viewport, visibility::RenderLayers},
    gizmos::transform_gizmo::{
        TransformGizmoAxis, TransformGizmoCamera, TransformGizmoFocus, TransformGizmoMode,
        TransformGizmoPlugin, TransformGizmoSettings, TransformGizmoSpace, TransformGizmoState,
        TransformGizmoSystems,
    },
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
    ui::{RelativeCursorPosition, UiSystems},
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};
use fluent_bundle::FluentArgs;
use std::time::Instant;

const DEFAULT_PREVIEW_PITCH: f32 = -0.35;
const PREVIEW_GRID_SHADER_PATH: &str = "shaders/preview_grid.wesl";
const PREVIEW_GRID_Y: f32 = -0.05;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ViewportSet {
    Setup,
    Update,
}

pub(crate) struct ViewportPlugin;

impl Plugin for ViewportPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TransformGizmoPlugin)
            .add_plugins(MaterialPlugin::<PreviewGridMaterial>::default())
            .init_gizmo_group::<PreviewSceneGizmos>()
            .init_resource::<PreviewCameraController>()
            .init_resource::<PreviewNavigationState>()
            .init_resource::<PreviewDisplayState>()
            .init_resource::<ShapeGizmoState>()
            .init_resource::<EmitterTransformGizmoInteraction>()
            .add_systems(
                Startup,
                (setup_preview_scene, configure_preview_scene_gizmos)
                    .chain()
                    .in_set(ViewportSet::Setup),
            )
            .add_systems(
                PostStartup,
                (
                    configure_transform_gizmo_overlay_camera,
                    configure_transform_gizmo_overlay_materials,
                ),
            )
            .add_systems(
                Update,
                (
                    sync_rendered_preview,
                    update_preview,
                    navigate_preview_camera,
                    sync_preview_grid,
                    sync_preview_display_mode,
                    update_preview_display_controls,
                    update_transform_gizmo_controls,
                    sync_emitter_transform_proxy,
                    interact_shape_gizmo,
                    sync_transform_gizmo_focus,
                    draw_preview_scene_gizmos,
                    update_viewport_status_label,
                )
                    .chain()
                    .in_set(ViewportSet::Update),
            )
            .add_systems(
                PostUpdate,
                (
                    sync_preview_camera_viewport
                        .after(UiSystems::Layout)
                        .before(TransformGizmoSystems),
                    update_emitter_transform_gizmo
                        .after(TransformGizmoSystems)
                        .before(TransformGizmoRenderStep),
                    apply_transform_gizmo_drag_feedback
                        .after(TransformGizmoRenderStep)
                        .after(update_emitter_transform_gizmo),
                ),
            )
            .configure_sets(Update, AestraSet::Playback.after(sync_rendered_preview));
    }
}

fn setup_preview_scene(
    mut commands: Commands,
    session: Res<EditorSession>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut grid_materials: ResMut<Assets<PreviewGridMaterial>>,
) {
    spawn_preview_camera(&mut commands);
    commands.spawn((
        PreviewGridPlane,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2.0, 2.0))),
        MeshMaterial3d(grid_materials.add(PreviewGridMaterial {
            grid: preview_grid_uniform(140.0, Vec3::ZERO, 1.0),
        })),
        Transform::from_xyz(0.0, PREVIEW_GRID_Y, 0.0).with_scale(Vec3::new(2_800.0, 1.0, 2_800.0)),
        RenderLayers::layer(0),
    ));
    spawn_preview_effect_player(&mut commands, &session, Transform::IDENTITY);
    commands.spawn((
        EmitterTransformGizmoProxy,
        TransformGizmoFocus,
        bevy_transform_from_emitter(session.selected_layer().transform),
    ));
}

#[derive(Component)]
struct PreviewCanvas;

#[derive(Component)]
pub(crate) struct PreviewRenderCamera;

#[derive(Component)]
struct PreviewGridPlane;

#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
struct PreviewGridUniform {
    minor_color: Vec4,
    major_color: Vec4,
    x_axis_color: Vec4,
    z_axis_color: Vec4,
    parameters: Vec4,
    focus: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct PreviewGridMaterial {
    #[uniform(0)]
    grid: PreviewGridUniform,
}

impl Material for PreviewGridMaterial {
    fn fragment_shader() -> ShaderRef {
        PREVIEW_GRID_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn depth_bias(&self) -> f32 {
        100.0
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Resource)]
pub(crate) struct PreviewCameraController {
    focus: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
    frame_requested: bool,
}

impl Default for PreviewCameraController {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            distance: 140.0,
            yaw: 0.0,
            pitch: DEFAULT_PREVIEW_PITCH,
            frame_requested: false,
        }
    }
}

impl PreviewCameraController {
    fn frame_effect(&mut self, position: Vec3) {
        self.focus = position;
        self.distance = 140.0;
        self.yaw = 0.0;
        self.pitch = DEFAULT_PREVIEW_PITCH;
        self.frame_requested = false;
    }

    pub(crate) fn request_frame(&mut self) {
        self.frame_requested = true;
    }
}

#[derive(Resource, Default)]
struct PreviewNavigationState {
    dragging: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PreviewDisplayMode {
    Wireframe,
    #[default]
    Rendered,
}

#[derive(Default, Reflect, GizmoConfigGroup)]
struct PreviewSceneGizmos;

#[derive(Resource, Default)]
pub(crate) struct PreviewDisplayState {
    mode: PreviewDisplayMode,
}

impl PreviewDisplayState {
    pub(crate) fn set_mode(&mut self, mode: PreviewDisplayMode) {
        self.mode = mode;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShapeGizmoHandle {
    Radius,
    Depth,
    ExtentX,
    ExtentY,
    ExtentZ,
}

#[derive(Clone, Copy, Debug)]
struct ActiveShapeGizmoDrag {
    emitter: EmitterId,
    module: ModuleId,
    handle: ShapeGizmoHandle,
    original: EmitterShape,
    current: EmitterShape,
}

#[derive(Resource, Default)]
struct ShapeGizmoState {
    hovered: Option<ShapeGizmoHandle>,
    active: Option<ActiveShapeGizmoDrag>,
}

#[derive(Component)]
struct PreviewDisplayModeIcon(PreviewDisplayMode);

#[derive(Component)]
struct TransformGizmoModeFill(TransformGizmoMode);

#[derive(Component)]
struct TransformGizmoModeOutline(TransformGizmoMode);

#[derive(Component)]
struct PreviewEffectPlayer;

#[derive(Component)]
pub(crate) struct EmitterTransformGizmoProxy;

#[derive(Component)]
struct TransformGizmoVisualRoot;

#[derive(Clone, Copy, Debug)]
struct ActiveEmitterTransformGizmo {
    emitter: EmitterId,
    original: EmitterTransform,
    current: EmitterTransform,
}

#[derive(Resource, Default)]
pub(crate) struct EmitterTransformGizmoInteraction {
    active: Option<ActiveEmitterTransformGizmo>,
}

impl EmitterTransformGizmoInteraction {
    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

#[derive(Component)]
struct GizmoModeLabel;

#[derive(Component)]
struct ShapeGizmoValueLabel;

#[derive(Component)]
struct ParticleCountLabel;

fn spawn_preview_camera(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            PreviewRenderCamera,
            TransformGizmoCamera,
            Camera3d::default(),
            Camera {
                order: -2,
                clear_color: ClearColorConfig::Custom(theme::VIEWPORT),
                viewport: Some(Viewport {
                    physical_size: UVec2::splat(128),
                    ..default()
                }),
                ..default()
            },
            preview_camera_transform(Vec3::ZERO, 140.0, 0.0, DEFAULT_PREVIEW_PITCH),
            RenderLayers::layer(0),
        ))
        .id()
}

fn configure_transform_gizmo_overlay_camera(
    mut cameras: Query<
        (&RenderLayers, &mut Camera),
        (With<Camera3d>, Without<PreviewRenderCamera>),
    >,
) {
    let gizmo_layers = RenderLayers::layer(15);
    for (layers, mut camera) in &mut cameras {
        if layers == &gizmo_layers {
            camera.clear_color = ClearColorConfig::None;
            camera.order = 1;
            camera.is_active = true;
        }
    }
}

fn configure_transform_gizmo_overlay_materials(
    mut commands: Commands,
    gizmo_meshes: Query<(&RenderLayers, &MeshMaterial3d<StandardMaterial>, &ChildOf)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let gizmo_layers = RenderLayers::layer(15);
    for (layers, material, parent) in &gizmo_meshes {
        if layers != &gizmo_layers {
            continue;
        }
        commands
            .entity(parent.parent())
            .insert(TransformGizmoVisualRoot);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.alpha_mode = AlphaMode::Blend;
            material.depth_bias = 10_000.0;
        }
    }
}

fn sync_preview_camera_viewport(
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<PreviewCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut preview_camera: Single<&mut Camera, With<PreviewRenderCamera>>,
    mut overlay_cameras: Query<
        (&RenderLayers, &mut Camera),
        (With<Camera3d>, Without<PreviewRenderCamera>),
    >,
) {
    let Ok((computed, transform)) = canvas.single() else {
        set_preview_cameras_active(&mut preview_camera, &mut overlay_cameras, false);
        return;
    };
    let size = computed.size();
    if !size.is_finite() || size.x < 16.0 || size.y < 16.0 {
        set_preview_cameras_active(&mut preview_camera, &mut overlay_cameras, false);
        return;
    }
    set_preview_cameras_active(&mut preview_camera, &mut overlay_cameras, true);
    let top_left = transform.translation.trunc() - size * 0.5;
    let target_size = UVec2::new(
        window.physical_width().max(1),
        window.physical_height().max(1),
    );
    let position = top_left.max(Vec2::ZERO).as_uvec2().min(target_size - 1);
    let available = target_size.saturating_sub(position).max(UVec2::ONE);
    let physical_size = size.as_uvec2().max(UVec2::ONE).min(available);
    preview_camera.viewport = Some(Viewport {
        physical_position: position,
        physical_size,
        ..default()
    });
}

fn set_preview_cameras_active(
    preview_camera: &mut Camera,
    overlay_cameras: &mut Query<
        (&RenderLayers, &mut Camera),
        (With<Camera3d>, Without<PreviewRenderCamera>),
    >,
    active: bool,
) {
    preview_camera.is_active = active;
    let gizmo_layers = RenderLayers::layer(15);
    for (layers, mut camera) in overlay_cameras {
        if layers == &gizmo_layers {
            camera.is_active = active;
        }
    }
}

fn navigate_preview_camera(
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    canvas: Single<&RelativeCursorPosition, With<PreviewCanvas>>,
    player: Query<&GlobalTransform, With<EmitterTransformGizmoProxy>>,
    mut navigation: ResMut<PreviewNavigationState>,
    mut controller: ResMut<PreviewCameraController>,
    mut camera: Single<&mut Transform, With<PreviewRenderCamera>>,
) {
    let cursor_over = canvas.cursor_over();
    let pointer_delta = motion
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let scroll_delta = wheel.read().fold(0.0, |sum, event| {
        let scale = match event.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.02,
        };
        sum + event.y * scale
    });
    if buttons.just_pressed(MouseButton::Middle) && cursor_over {
        navigation.dragging = true;
    }
    if buttons.just_released(MouseButton::Middle) {
        navigation.dragging = false;
    }
    if cursor_over && (keys.just_pressed(KeyCode::KeyF) || keys.just_pressed(KeyCode::Home)) {
        controller.frame_requested = true;
    }

    let mut changed = false;
    if controller.frame_requested {
        let focus = player
            .single()
            .map_or(Vec3::ZERO, GlobalTransform::translation);
        controller.frame_effect(focus);
        changed = true;
    }
    if navigation.dragging && buttons.pressed(MouseButton::Middle) && pointer_delta != Vec2::ZERO {
        if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
            controller.distance =
                (controller.distance * (pointer_delta.y * 0.01).exp()).clamp(1.0, 4_000.0);
        } else if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            let right = camera.rotation * Vec3::X;
            let up = camera.rotation * Vec3::Y;
            let units_per_pixel = controller.distance * 0.0018;
            controller.focus += (-right * pointer_delta.x + up * pointer_delta.y) * units_per_pixel;
        } else {
            controller.yaw -= pointer_delta.x * 0.005;
            controller.pitch = (controller.pitch - pointer_delta.y * 0.005).clamp(-1.54, 1.54);
        }
        changed = true;
    }
    if cursor_over && scroll_delta != 0.0 {
        controller.distance =
            (controller.distance * (-scroll_delta * 0.12).exp()).clamp(1.0, 4_000.0);
        changed = true;
    }
    if !changed {
        return;
    }

    **camera = preview_camera_transform(
        controller.focus,
        controller.distance,
        controller.yaw,
        controller.pitch,
    );
}

fn preview_camera_transform(focus: Vec3, distance: f32, yaw: f32, pitch: f32) -> Transform {
    let orbit = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
    Transform::from_translation(focus + orbit * Vec3::Z * distance).looking_at(focus, Vec3::Y)
}

fn sync_preview_grid(
    menu: Res<MenuState>,
    controller: Res<PreviewCameraController>,
    camera: Single<&GlobalTransform, With<PreviewRenderCamera>>,
    grid: Single<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<PreviewGridMaterial>,
        ),
        With<PreviewGridPlane>,
    >,
    mut materials: ResMut<Assets<PreviewGridMaterial>>,
) {
    let (mut transform, mut visibility, material_handle) = grid.into_inner();
    *visibility = if menu.show_grid {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };

    let plane_radius = (controller.distance * 24.0).clamp(200.0, 100_000.0);
    transform.translation = Vec3::new(controller.focus.x, PREVIEW_GRID_Y, controller.focus.z);
    transform.scale = Vec3::new(plane_radius, 1.0, plane_radius);

    let view_angle = camera.forward().dot(Vec3::NEG_Y).abs();
    let angle_fade = ((view_angle - 0.015) / 0.16).clamp(0.0, 1.0);
    if let Some(mut material) = materials.get_mut(&material_handle.0) {
        let uniform = preview_grid_uniform(controller.distance, controller.focus, angle_fade);
        if material.grid != uniform {
            material.grid = uniform;
        }
    }
}

fn preview_grid_uniform(distance: f32, focus: Vec3, angle_fade: f32) -> PreviewGridUniform {
    PreviewGridUniform {
        minor_color: Vec4::new(0.20, 0.22, 0.29, 0.22),
        major_color: Vec4::new(0.27, 0.30, 0.39, 0.38),
        x_axis_color: Vec4::new(0.58, 0.20, 0.27, 0.62),
        z_axis_color: Vec4::new(0.20, 0.40, 0.68, 0.62),
        parameters: Vec4::new(
            adaptive_grid_spacing(distance / 18.0),
            (distance * 8.0).max(90.0),
            angle_fade,
            10.0,
        ),
        focus: Vec4::new(focus.x, focus.z, 0.0, 0.0),
    }
}

fn sync_preview_display_mode(
    display: Res<PreviewDisplayState>,
    mut players: Query<(&mut EffectPlayer, &mut Visibility), With<PreviewEffectPlayer>>,
) {
    let render_mode = match display.mode {
        PreviewDisplayMode::Wireframe => EffectRenderMode::Wireframe,
        PreviewDisplayMode::Rendered => EffectRenderMode::Rendered,
    };
    for (mut player, mut visibility) in &mut players {
        player.set_render_mode(render_mode);
        *visibility = Visibility::Inherited;
    }
}

fn update_preview_display_controls(
    display: Res<PreviewDisplayState>,
    mut icons: Query<(
        &PreviewDisplayModeIcon,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (icon, mut background, mut border) in &mut icons {
        let active = icon.0 == display.mode;
        match icon.0 {
            PreviewDisplayMode::Wireframe => {
                background.0 = Color::NONE;
                *border = BorderColor::all(if active {
                    theme::ACCENT
                } else {
                    theme::TEXT_MUTED
                });
            }
            PreviewDisplayMode::Rendered => {
                background.0 = if active {
                    theme::ACCENT
                } else {
                    theme::TEXT_MUTED
                };
                *border = BorderColor::all(Color::NONE);
            }
        }
    }
}

fn update_transform_gizmo_controls(
    keys: Res<ButtonInput<KeyCode>>,
    canvas: Single<&RelativeCursorPosition, With<PreviewCanvas>>,
    mut settings: ResMut<TransformGizmoSettings>,
    mut labels: Query<&mut Text, With<GizmoModeLabel>>,
    mut fills: Query<(&TransformGizmoModeFill, &mut BackgroundColor)>,
    mut outlines: Query<(&TransformGizmoModeOutline, &mut BorderColor)>,
) {
    if canvas.cursor_over() {
        if keys.just_pressed(KeyCode::Digit1) {
            settings.mode = TransformGizmoMode::Translate;
        }
        if keys.just_pressed(KeyCode::Digit2) {
            settings.mode = TransformGizmoMode::Rotate;
        }
        if keys.just_pressed(KeyCode::Digit3) {
            settings.mode = TransformGizmoMode::Scale;
        }
        if keys.just_pressed(KeyCode::KeyX) {
            settings.space = match settings.space {
                TransformGizmoSpace::World => TransformGizmoSpace::Local,
                TransformGizmoSpace::Local => TransformGizmoSpace::World,
            };
        }
    }
    let mode = match settings.mode {
        TransformGizmoMode::Translate => "MOVE",
        TransformGizmoMode::Rotate => "ROTATE",
        TransformGizmoMode::Scale => "SCALE",
    };
    let space = match settings.space {
        TransformGizmoSpace::World => "WORLD",
        TransformGizmoSpace::Local => "LOCAL",
    };
    for mut label in &mut labels {
        **label = format!("{mode} · {space}");
    }
    for (icon, mut color) in &mut fills {
        color.0 = if icon.0 == settings.mode {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        };
    }
    for (icon, mut color) in &mut outlines {
        *color = BorderColor::all(if icon.0 == settings.mode {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        });
    }
}

fn sync_transform_gizmo_focus(
    mut commands: Commands,
    session: Res<EditorSession>,
    shape_gizmo: Res<ShapeGizmoState>,
    proxies: Query<(Entity, Has<TransformGizmoFocus>), With<EmitterTransformGizmoProxy>>,
) {
    let emitter = session.selected_layer().id;
    let allowed = session.pending_change.is_none()
        && shape_gizmo.hovered.is_none()
        && shape_gizmo.active.is_none()
        && !session
            .locks
            .is_locked(SemanticTarget::Effect(session.effect.id))
        && !session.locks.is_locked(SemanticTarget::Emitter(emitter));
    for (entity, has_focus) in &proxies {
        if allowed && !has_focus {
            commands.entity(entity).insert(TransformGizmoFocus);
        } else if !allowed && has_focus {
            commands.entity(entity).remove::<TransformGizmoFocus>();
        }
    }
}

fn sync_emitter_transform_proxy(
    session: Res<EditorSession>,
    gizmo: Res<TransformGizmoState>,
    interaction: Res<EmitterTransformGizmoInteraction>,
    mut proxies: Query<&mut Transform, With<EmitterTransformGizmoProxy>>,
) {
    if gizmo.active || interaction.active.is_some() {
        return;
    }
    let desired = bevy_transform_from_emitter(session.selected_layer().transform);
    for mut transform in &mut proxies {
        if *transform != desired {
            *transform = desired;
        }
    }
}

fn update_emitter_transform_gizmo(
    gizmo: Res<TransformGizmoState>,
    mut interaction: ResMut<EmitterTransformGizmoInteraction>,
    proxies: Query<&Transform, With<EmitterTransformGizmoProxy>>,
    mut session: ResMut<EditorSession>,
) {
    let Ok(transform) = proxies.single() else {
        return;
    };
    let current = emitter_transform_from_bevy(transform);
    if gizmo.active {
        let active = interaction
            .active
            .get_or_insert_with(|| ActiveEmitterTransformGizmo {
                emitter: session.selected_layer().id,
                original: session.selected_layer().transform,
                current: session.selected_layer().transform,
            });
        if active.current != current {
            active.current = current;
            session.preview_interaction(EffectTransaction::single(
                "Preview emitter transform",
                EffectCommand::SetEmitterTransform {
                    id: active.emitter,
                    transform: current,
                },
            ));
        }
        return;
    }

    let Some(active) = interaction.active.take() else {
        return;
    };
    if active.current != active.original {
        if !session.execute(
            "Transformed emitter",
            EffectCommand::SetEmitterTransform {
                id: active.emitter,
                transform: active.current,
            },
            true,
        ) {
            session.restore_interaction_preview();
        }
    } else {
        session.restore_interaction_preview();
    }
}

#[derive(Clone, Copy, Debug)]
struct TransformGizmoDragVisual {
    rotation: Option<Quat>,
    scale: Vec3,
}

fn transform_gizmo_drag_visual(
    mode: TransformGizmoMode,
    space: TransformGizmoSpace,
    axis: Option<TransformGizmoAxis>,
    original: Transform,
    current: Transform,
) -> TransformGizmoDragVisual {
    let mut visual = TransformGizmoDragVisual {
        rotation: None,
        scale: Vec3::ONE,
    };
    match mode {
        TransformGizmoMode::Translate => {}
        TransformGizmoMode::Rotate => {
            visual.rotation = Some(match space {
                TransformGizmoSpace::World => {
                    (current.rotation * original.rotation.inverse()).normalize()
                }
                TransformGizmoSpace::Local => current.rotation.normalize(),
            });
        }
        TransformGizmoMode::Scale => {
            let ratio = (current.scale / original.scale.max(Vec3::splat(0.001)))
                .abs()
                .clamp(Vec3::splat(0.15), Vec3::splat(6.0));
            visual.scale = match axis {
                Some(TransformGizmoAxis::X) => Vec3::new(ratio.x, 1.0, 1.0),
                Some(TransformGizmoAxis::Y) => Vec3::new(1.0, ratio.y, 1.0),
                Some(TransformGizmoAxis::Z) => Vec3::new(1.0, 1.0, ratio.z),
                Some(TransformGizmoAxis::View) | None => ratio,
            };
        }
    }
    visual
}

fn apply_transform_gizmo_drag_feedback(
    gizmo: Res<TransformGizmoState>,
    settings: Res<TransformGizmoSettings>,
    interaction: Res<EmitterTransformGizmoInteraction>,
    proxy: Single<&Transform, With<EmitterTransformGizmoProxy>>,
    mut roots: Query<
        (Entity, &mut Transform, &mut GlobalTransform),
        (
            With<TransformGizmoVisualRoot>,
            Without<EmitterTransformGizmoProxy>,
        ),
    >,
    mut children: Query<
        (&ChildOf, &Transform, &mut GlobalTransform),
        (
            With<Mesh3d>,
            Without<TransformGizmoVisualRoot>,
            Without<EmitterTransformGizmoProxy>,
        ),
    >,
) {
    if !gizmo.active {
        return;
    }
    let Some(active) = interaction.active else {
        return;
    };
    let visual = transform_gizmo_drag_visual(
        settings.mode,
        settings.space,
        gizmo.axis,
        bevy_transform_from_emitter(active.original),
        **proxy,
    );
    for (root_entity, mut root, mut root_global) in &mut roots {
        if let Some(rotation) = visual.rotation {
            root.rotation = rotation;
        }
        root.scale *= visual.scale;
        *root_global = GlobalTransform::from(*root);
        for (parent, local, mut global) in &mut children {
            if parent.parent() == root_entity {
                *global = root_global.mul_transform(*local);
            }
        }
    }
}

fn bevy_transform_from_emitter(transform: EmitterTransform) -> Transform {
    Transform {
        translation: Vec3::from_array(transform.translation),
        rotation: Quat::from_array(transform.rotation).normalize(),
        scale: Vec3::from_array(transform.scale),
    }
}

pub(crate) fn emitter_transform_from_bevy(transform: &Transform) -> EmitterTransform {
    EmitterTransform {
        translation: transform.translation.to_array(),
        rotation: transform.rotation.normalize().to_array(),
        scale: transform.scale.max(Vec3::splat(0.001)).to_array(),
    }
}

#[allow(clippy::too_many_arguments)]
fn interact_shape_gizmo(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    menu: Res<MenuState>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cursor_icon: Single<&mut CursorIcon, With<PrimaryWindow>>,
    canvas: Single<
        (&RelativeCursorPosition, &ComputedNode, &UiGlobalTransform),
        With<PreviewCanvas>,
    >,
    camera: Single<(&Camera, &GlobalTransform), With<PreviewRenderCamera>>,
    players: Query<&GlobalTransform, With<EmitterTransformGizmoProxy>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<ShapeGizmoState>,
    mut labels: Query<(&mut Text, &mut Node, &mut Visibility), With<ShapeGizmoValueLabel>>,
    localizer: Res<Localizer>,
) {
    let was_using_cursor = state.hovered.is_some() || state.active.is_some();
    let cursor_position = window.cursor_position();
    let selected = selected_shape_module(&session);
    let Ok(player) = players.single() else {
        state.hovered = None;
        state.active = None;
        update_shape_gizmo_label(&mut labels, None, None, canvas.1, canvas.2, &localizer);
        return;
    };
    let (camera, camera_transform) = *camera;

    if keys.just_pressed(KeyCode::Escape) && state.active.take().is_some() {
        state.hovered = None;
        **cursor_icon = CursorIcon::System(SystemCursorIcon::Default);
        update_shape_gizmo_label(&mut labels, None, None, canvas.1, canvas.2, &localizer);
        session.restore_interaction_preview();
        session.status = "Cancelled shape adjustment".into();
        return;
    }

    if let Some(active) = state.active.as_mut() {
        if !buttons.pressed(MouseButton::Left) {
            let active = state.active.take().expect("shape drag is active");
            state.hovered = None;
            **cursor_icon = CursorIcon::System(SystemCursorIcon::Default);
            update_shape_gizmo_label(&mut labels, None, None, canvas.1, canvas.2, &localizer);
            if active.current != active.original {
                if !session.execute(
                    "Adjusted spawn shape",
                    EffectCommand::SetModuleParameter {
                        emitter: active.emitter,
                        module: active.module,
                        parameter: "shape".into(),
                        value: Value::Shape(active.current),
                    },
                    true,
                ) {
                    session.restore_interaction_preview();
                }
            } else {
                session.restore_interaction_preview();
            }
            return;
        }
        if let Some(cursor_position) = cursor_position
            && let Some(value) = shape_gizmo_drag_value(
                camera,
                camera_transform,
                player,
                active.original,
                active.handle,
                cursor_position,
            )
        {
            let next = shape_after_gizmo_drag(active.original, active.handle, value);
            if next != active.current {
                active.current = next;
                session.preview_interaction(EffectTransaction::single(
                    "Preview shape adjustment",
                    EffectCommand::SetModuleParameter {
                        emitter: active.emitter,
                        module: active.module,
                        parameter: "shape".into(),
                        value: Value::Shape(active.current),
                    },
                ));
            }
        }
        **cursor_icon = shape_gizmo_cursor(active.handle);
        update_shape_gizmo_label(
            &mut labels,
            cursor_position,
            Some((active.handle, active.current)),
            canvas.1,
            canvas.2,
            &localizer,
        );
        return;
    }

    state.hovered = if menu.open.is_none()
        && menu.tab_context.is_none()
        && canvas.0.cursor_over()
        && session.pending_change.is_none()
    {
        cursor_position.and_then(|cursor_position| {
            selected.and_then(|selected| {
                hit_test_shape_gizmo(
                    camera,
                    camera_transform,
                    player,
                    selected.shape,
                    cursor_position,
                )
            })
        })
    } else {
        None
    };

    if let Some(handle) = state.hovered {
        **cursor_icon = shape_gizmo_cursor(handle);
        if buttons.just_pressed(MouseButton::Left)
            && let Some(selected) = selected
            && !shape_gizmo_target_locked(&session, selected)
        {
            state.active = Some(ActiveShapeGizmoDrag {
                emitter: selected.emitter,
                module: selected.module,
                handle,
                original: selected.shape,
                current: selected.shape,
            });
            update_shape_gizmo_label(
                &mut labels,
                cursor_position,
                Some((handle, selected.shape)),
                canvas.1,
                canvas.2,
                &localizer,
            );
        }
    } else if was_using_cursor {
        **cursor_icon = CursorIcon::System(SystemCursorIcon::Default);
    }
    if state.active.is_none() {
        update_shape_gizmo_label(&mut labels, None, None, canvas.1, canvas.2, &localizer);
    }
}

fn shape_gizmo_target_locked(session: &EditorSession, selected: SelectedShapeModule) -> bool {
    session
        .locks
        .is_locked(SemanticTarget::Effect(session.effect.id))
        || session
            .locks
            .is_locked(SemanticTarget::Emitter(selected.emitter))
        || session
            .locks
            .is_locked(SemanticTarget::Module(selected.module))
}

fn shape_gizmo_cursor(handle: ShapeGizmoHandle) -> CursorIcon {
    CursorIcon::System(match handle {
        ShapeGizmoHandle::Radius => SystemCursorIcon::EwResize,
        ShapeGizmoHandle::Depth => SystemCursorIcon::NsResize,
        ShapeGizmoHandle::ExtentX => SystemCursorIcon::EwResize,
        ShapeGizmoHandle::ExtentY => SystemCursorIcon::NsResize,
        ShapeGizmoHandle::ExtentZ => SystemCursorIcon::NeswResize,
    })
}

fn shape_handle_local_positions(shape: EmitterShape) -> Vec<(ShapeGizmoHandle, Vec3)> {
    match shape {
        EmitterShape::Point => Vec::new(),
        EmitterShape::Circle { radius } | EmitterShape::Ring { radius } => {
            vec![(ShapeGizmoHandle::Radius, Vec3::X * radius)]
        }
        EmitterShape::Sphere { radius } | EmitterShape::Hemisphere { radius } => {
            vec![(ShapeGizmoHandle::Radius, Vec3::X * radius)]
        }
        EmitterShape::Box { half_extents } => vec![
            (ShapeGizmoHandle::ExtentX, Vec3::X * half_extents[0]),
            (ShapeGizmoHandle::ExtentY, Vec3::Y * half_extents[1]),
            (ShapeGizmoHandle::ExtentZ, Vec3::Z * half_extents[2]),
        ],
        EmitterShape::Cylinder { radius, depth } => vec![
            (ShapeGizmoHandle::Radius, Vec3::X * radius),
            (ShapeGizmoHandle::Depth, Vec3::Y * depth * 0.5),
        ],
        EmitterShape::Cone { radius, depth } => vec![
            (ShapeGizmoHandle::Radius, Vec3::new(radius, depth, 0.0)),
            (ShapeGizmoHandle::Depth, Vec3::new(0.0, depth, 0.0)),
        ],
    }
}

fn hit_test_shape_gizmo(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    player: &GlobalTransform,
    shape: EmitterShape,
    cursor_position: Vec2,
) -> Option<ShapeGizmoHandle> {
    shape_handle_local_positions(shape)
        .into_iter()
        .filter_map(|(handle, local_position)| {
            let viewport_position = camera
                .world_to_viewport(camera_transform, player.transform_point(local_position))
                .ok()?;
            let distance = viewport_position.distance(cursor_position);
            (distance <= 11.0).then_some((handle, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(handle, _)| handle)
}

fn shape_gizmo_drag_value(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    player: &GlobalTransform,
    shape: EmitterShape,
    handle: ShapeGizmoHandle,
    cursor_position: Vec2,
) -> Option<f32> {
    let (axis, multiplier, anchor) = match handle {
        ShapeGizmoHandle::Radius => {
            let anchor = match shape {
                EmitterShape::Cone { depth, .. } => Vec3::Y * depth,
                _ => Vec3::ZERO,
            };
            (Vec3::X, 1.0, anchor)
        }
        ShapeGizmoHandle::Depth => match shape {
            EmitterShape::Cylinder { .. } => (Vec3::Y, 2.0, Vec3::ZERO),
            _ => (Vec3::Y, 1.0, Vec3::ZERO),
        },
        ShapeGizmoHandle::ExtentX => (Vec3::X, 1.0, Vec3::ZERO),
        ShapeGizmoHandle::ExtentY => (Vec3::Y, 1.0, Vec3::ZERO),
        ShapeGizmoHandle::ExtentZ => (Vec3::Z, 1.0, Vec3::ZERO),
    };
    let screen_origin = camera
        .world_to_viewport(camera_transform, player.transform_point(anchor))
        .ok()?;
    let screen_axis = camera
        .world_to_viewport(camera_transform, player.transform_point(anchor + axis))
        .ok()?
        - screen_origin;
    let pixels_per_unit = screen_axis.length();
    if pixels_per_unit < 1.0e-4 {
        return None;
    }
    Some(
        ((cursor_position - screen_origin).dot(screen_axis / pixels_per_unit) / pixels_per_unit)
            .abs()
            * multiplier,
    )
}

fn shape_after_gizmo_drag(
    original: EmitterShape,
    handle: ShapeGizmoHandle,
    value: f32,
) -> EmitterShape {
    const MIN_HANDLE_VALUE: f32 = 0.1;
    match (original, handle) {
        (EmitterShape::Circle { .. }, ShapeGizmoHandle::Radius) => EmitterShape::Circle {
            radius: value.max(MIN_HANDLE_VALUE),
        },
        (EmitterShape::Ring { .. }, ShapeGizmoHandle::Radius) => EmitterShape::Ring {
            radius: value.max(MIN_HANDLE_VALUE),
        },
        (EmitterShape::Sphere { .. }, ShapeGizmoHandle::Radius) => EmitterShape::Sphere {
            radius: value.max(MIN_HANDLE_VALUE),
        },
        (EmitterShape::Hemisphere { .. }, ShapeGizmoHandle::Radius) => EmitterShape::Hemisphere {
            radius: value.max(MIN_HANDLE_VALUE),
        },
        (EmitterShape::Box { mut half_extents }, ShapeGizmoHandle::ExtentX) => {
            half_extents[0] = value.max(MIN_HANDLE_VALUE);
            EmitterShape::Box { half_extents }
        }
        (EmitterShape::Box { mut half_extents }, ShapeGizmoHandle::ExtentY) => {
            half_extents[1] = value.max(MIN_HANDLE_VALUE);
            EmitterShape::Box { half_extents }
        }
        (EmitterShape::Box { mut half_extents }, ShapeGizmoHandle::ExtentZ) => {
            half_extents[2] = value.max(MIN_HANDLE_VALUE);
            EmitterShape::Box { half_extents }
        }
        (EmitterShape::Cylinder { depth, .. }, ShapeGizmoHandle::Radius) => {
            EmitterShape::Cylinder {
                radius: value.max(MIN_HANDLE_VALUE),
                depth,
            }
        }
        (EmitterShape::Cylinder { radius, .. }, ShapeGizmoHandle::Depth) => {
            EmitterShape::Cylinder {
                radius,
                depth: value.max(MIN_HANDLE_VALUE),
            }
        }
        (EmitterShape::Cone { depth, .. }, ShapeGizmoHandle::Radius) => EmitterShape::Cone {
            radius: value.max(MIN_HANDLE_VALUE),
            depth,
        },
        (EmitterShape::Cone { radius, .. }, ShapeGizmoHandle::Depth) => EmitterShape::Cone {
            radius,
            depth: value.max(MIN_HANDLE_VALUE),
        },
        (shape, _) => shape,
    }
}

fn update_shape_gizmo_label(
    labels: &mut Query<(&mut Text, &mut Node, &mut Visibility), With<ShapeGizmoValueLabel>>,
    cursor_position: Option<Vec2>,
    value: Option<(ShapeGizmoHandle, EmitterShape)>,
    canvas: &ComputedNode,
    canvas_transform: &UiGlobalTransform,
    localizer: &Localizer,
) {
    let Some((handle, shape)) = value else {
        for (_, _, mut visibility) in labels {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let scalar = match (handle, shape) {
        (ShapeGizmoHandle::Radius, EmitterShape::Circle { radius })
        | (ShapeGizmoHandle::Radius, EmitterShape::Ring { radius })
        | (ShapeGizmoHandle::Radius, EmitterShape::Sphere { radius })
        | (ShapeGizmoHandle::Radius, EmitterShape::Hemisphere { radius })
        | (ShapeGizmoHandle::Radius, EmitterShape::Cylinder { radius, .. })
        | (ShapeGizmoHandle::Radius, EmitterShape::Cone { radius, .. }) => radius,
        (ShapeGizmoHandle::Depth, EmitterShape::Cone { depth, .. })
        | (ShapeGizmoHandle::Depth, EmitterShape::Cylinder { depth, .. }) => depth,
        (ShapeGizmoHandle::ExtentX, EmitterShape::Box { half_extents }) => half_extents[0],
        (ShapeGizmoHandle::ExtentY, EmitterShape::Box { half_extents }) => half_extents[1],
        (ShapeGizmoHandle::ExtentZ, EmitterShape::Box { half_extents }) => half_extents[2],
        _ => return,
    };
    let mut args = FluentArgs::new();
    args.set("value", format!("{scalar:.2}"));
    let message = localizer.text_with(
        match handle {
            ShapeGizmoHandle::Radius => "viewport-shape-radius",
            ShapeGizmoHandle::Depth => "viewport-shape-depth",
            ShapeGizmoHandle::ExtentX => "viewport-shape-extent-x",
            ShapeGizmoHandle::ExtentY => "viewport-shape-extent-y",
            ShapeGizmoHandle::ExtentZ => "viewport-shape-extent-z",
        },
        &args,
    );
    let top_left = canvas_transform.translation.trunc() - canvas.size() * 0.5;
    let local_position = cursor_position.unwrap_or(top_left + Vec2::splat(24.0)) - top_left;
    for (mut text, mut node, mut visibility) in labels {
        text.0.clone_from(&message);
        node.left = Val::Px((local_position.x + 14.0).clamp(8.0, canvas.size().x - 120.0));
        node.top = Val::Px((local_position.y + 14.0).clamp(8.0, canvas.size().y - 32.0));
        *visibility = Visibility::Inherited;
    }
}

fn draw_preview_scene_gizmos(
    session: Res<EditorSession>,
    state: Res<ShapeGizmoState>,
    camera: Single<(&Camera, &GlobalTransform), With<PreviewRenderCamera>>,
    players: Query<&GlobalTransform, With<EmitterTransformGizmoProxy>>,
    mut gizmos: Gizmos<PreviewSceneGizmos>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    if let Some(selected) = selected_shape_module(&session) {
        let shape = state
            .active
            .filter(|active| active.module == selected.module)
            .map_or(selected.shape, |active| active.current);
        draw_emitter_shape_gizmo(&mut gizmos, player, shape, theme::ACCENT.with_alpha(0.9));
        for (handle, local_position) in shape_handle_local_positions(shape) {
            let highlighted = state.active.is_some_and(|active| active.handle == handle)
                || state.hovered == Some(handle);
            let world_position = player.transform_point(local_position);
            let handle_radius = screen_space_gizmo_radius(
                camera.0,
                camera.1,
                world_position,
                if highlighted { 7.5 } else { 6.0 },
            );
            gizmos.sphere(
                world_position,
                handle_radius,
                if highlighted {
                    theme::TEXT
                } else {
                    theme::ACCENT
                },
            );
        }
    }
}

fn adaptive_grid_spacing(desired: f32) -> f32 {
    if !desired.is_finite() || desired <= f32::EPSILON {
        return 1.0;
    }
    let magnitude = 10.0_f32.powf(desired.log10().floor());
    let normalized = desired / magnitude;
    let multiplier = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    magnitude * multiplier
}

fn screen_space_gizmo_radius(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    world_position: Vec3,
    radius_pixels: f32,
) -> f32 {
    let camera_right = camera_transform.rotation() * Vec3::X;
    let Ok(center) = camera.world_to_viewport(camera_transform, world_position) else {
        return 0.01;
    };
    let Ok(unit_right) = camera.world_to_viewport(camera_transform, world_position + camera_right)
    else {
        return 0.01;
    };
    let pixels_per_world_unit = center.distance(unit_right);
    if !pixels_per_world_unit.is_finite() || pixels_per_world_unit <= 1.0e-4 {
        return 0.01;
    }
    (radius_pixels / pixels_per_world_unit).clamp(0.001, 1_000.0)
}

fn draw_emitter_shape_gizmo(
    gizmos: &mut Gizmos<PreviewSceneGizmos>,
    player: &GlobalTransform,
    shape: EmitterShape,
    color: Color,
) {
    let translation = player.translation();
    let rotation = player.rotation();
    let scale = player.to_scale_rotation_translation().0;
    let axis_scale = scale
        .x
        .abs()
        .max(scale.y.abs())
        .max(scale.z.abs())
        .max(0.001);
    let isometry = Isometry3d::new(translation, rotation);
    match shape {
        EmitterShape::Point => {
            gizmos.cross(isometry, 2.0 * axis_scale, color);
        }
        EmitterShape::Circle { radius } => {
            draw_local_ring(gizmos, player, Vec3::ZERO, radius, RingPlane::Xy, color);
            gizmos.line(
                player.transform_point(Vec3::ZERO),
                player.transform_point(Vec3::X * radius),
                color,
            );
        }
        EmitterShape::Ring { radius } => {
            draw_local_ring(gizmos, player, Vec3::ZERO, radius, RingPlane::Xy, color);
            draw_local_ring(
                gizmos,
                player,
                Vec3::ZERO,
                radius * 0.92,
                RingPlane::Xy,
                color.with_alpha(0.45),
            );
        }
        EmitterShape::Sphere { radius } => {
            for plane in [RingPlane::Xy, RingPlane::Xz, RingPlane::Yz] {
                draw_local_ring(gizmos, player, Vec3::ZERO, radius, plane, color);
            }
        }
        EmitterShape::Hemisphere { radius } => {
            draw_local_ring(gizmos, player, Vec3::ZERO, radius, RingPlane::Xz, color);
            for latitude in 1..=3 {
                let angle = latitude as f32 * std::f32::consts::FRAC_PI_2 / 4.0;
                draw_local_ring(
                    gizmos,
                    player,
                    Vec3::Y * radius * angle.sin(),
                    radius * angle.cos(),
                    RingPlane::Xz,
                    color.with_alpha(0.72),
                );
            }
            for longitude in 0..8 {
                let angle = longitude as f32 * std::f32::consts::TAU / 8.0;
                let mut previous = Vec3::new(radius * angle.cos(), 0.0, radius * angle.sin());
                for segment in 1..=16 {
                    let elevation = segment as f32 * std::f32::consts::FRAC_PI_2 / 16.0;
                    let next = Vec3::new(
                        radius * elevation.cos() * angle.cos(),
                        radius * elevation.sin(),
                        radius * elevation.cos() * angle.sin(),
                    );
                    gizmos.line(
                        player.transform_point(previous),
                        player.transform_point(next),
                        color.with_alpha(0.72),
                    );
                    previous = next;
                }
            }
        }
        EmitterShape::Box { half_extents } => {
            let half = Vec3::from_array(half_extents);
            for x in [-half.x, half.x] {
                for y in [-half.y, half.y] {
                    gizmos.line(
                        player.transform_point(Vec3::new(x, y, -half.z)),
                        player.transform_point(Vec3::new(x, y, half.z)),
                        color,
                    );
                }
            }
            for x in [-half.x, half.x] {
                for z in [-half.z, half.z] {
                    gizmos.line(
                        player.transform_point(Vec3::new(x, -half.y, z)),
                        player.transform_point(Vec3::new(x, half.y, z)),
                        color,
                    );
                }
            }
            for y in [-half.y, half.y] {
                for z in [-half.z, half.z] {
                    gizmos.line(
                        player.transform_point(Vec3::new(-half.x, y, z)),
                        player.transform_point(Vec3::new(half.x, y, z)),
                        color,
                    );
                }
            }
        }
        EmitterShape::Cylinder { radius, depth } => {
            let half_depth = depth * 0.5;
            draw_local_ring(
                gizmos,
                player,
                Vec3::Y * half_depth,
                radius,
                RingPlane::Xz,
                color,
            );
            draw_local_ring(
                gizmos,
                player,
                Vec3::Y * -half_depth,
                radius,
                RingPlane::Xz,
                color,
            );
            for quarter in 0..4 {
                let angle = quarter as f32 * std::f32::consts::FRAC_PI_2;
                let radial = Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
                gizmos.line(
                    player.transform_point(radial - Vec3::Y * half_depth),
                    player.transform_point(radial + Vec3::Y * half_depth),
                    color,
                );
            }
        }
        EmitterShape::Cone { radius, depth } => {
            let origin = player.transform_point(Vec3::ZERO);
            draw_local_ring(
                gizmos,
                player,
                Vec3::Y * depth,
                radius,
                RingPlane::Xz,
                color.with_alpha(0.72),
            );
            for quarter in 0..4 {
                let angle = quarter as f32 * std::f32::consts::FRAC_PI_2;
                gizmos.line(
                    origin,
                    player.transform_point(Vec3::new(
                        angle.cos() * radius,
                        depth,
                        angle.sin() * radius,
                    )),
                    color,
                );
            }
        }
    }
}

#[derive(Clone, Copy)]
enum RingPlane {
    Xy,
    Xz,
    Yz,
}

fn draw_local_ring(
    gizmos: &mut Gizmos<PreviewSceneGizmos>,
    player: &GlobalTransform,
    center: Vec3,
    radius: f32,
    plane: RingPlane,
    color: Color,
) {
    const SEGMENTS: usize = 64;
    let point = |index: usize| {
        let angle = index as f32 * std::f32::consts::TAU / SEGMENTS as f32;
        let radial = match plane {
            RingPlane::Xy => Vec3::new(angle.cos() * radius, angle.sin() * radius, 0.0),
            RingPlane::Xz => Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius),
            RingPlane::Yz => Vec3::new(0.0, angle.cos() * radius, angle.sin() * radius),
        };
        player.transform_point(center + radial)
    };
    for index in 0..SEGMENTS {
        gizmos.line(point(index), point((index + 1) % SEGMENTS), color);
    }
}

fn configure_preview_scene_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<PreviewSceneGizmos>();
    config.render_layers = RenderLayers::layer(15);
    config.line.width = 1.5;
}

fn configured_preview_player(session: &EditorSession) -> Option<EffectPlayer> {
    let preview = session.preview.as_ref()?;
    let mut player = EffectPlayer::from_compiled(preview.effect().clone());
    player.playing = false;
    player.speed = session.speed;
    player.set_seed(session.preview_seed);
    player.seek_frame(session.frame());
    Some(player)
}

fn spawn_preview_effect_player(
    commands: &mut Commands,
    session: &EditorSession,
    transform: Transform,
) {
    if let Some(player) = configured_preview_player(session) {
        commands.spawn((
            PreviewEffectPlayer,
            player,
            transform,
            RenderLayers::layer(0),
        ));
    }
}

pub(crate) fn spawn_preview(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    parent
        .spawn(())
        .apply_scene(ui_shell::viewport_pane())
        .with_children(|column| {
            column
                .spawn((
                    PreviewCanvas,
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(180.0),
                        position_type: PositionType::Relative,
                        overflow: Overflow::clip(),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    RelativeCursorPosition::default(),
                ))
                .with_children(|canvas| {
                    canvas.spawn((
                        Text::new("MOVE · WORLD"),
                        GizmoModeLabel,
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(12.0),
                            top: Val::Px(10.0),
                            ..default()
                        },
                    ));
                    canvas
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(8.0),
                                top: Val::Px(28.0),
                                padding: UiRect::all(Val::Px(2.0)),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(2.0),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(theme::PANEL.with_alpha(0.92)),
                            BorderColor::all(theme::BORDER),
                        ))
                        .with_children(|tools| {
                            spawn_transform_gizmo_tool_button(
                                tools,
                                TransformGizmoMode::Translate,
                                "viewport-gizmo-move",
                                "viewport-gizmo-move-description",
                                localizer,
                            );
                            spawn_transform_gizmo_tool_button(
                                tools,
                                TransformGizmoMode::Rotate,
                                "viewport-gizmo-rotate",
                                "viewport-gizmo-rotate-description",
                                localizer,
                            );
                            spawn_transform_gizmo_tool_button(
                                tools,
                                TransformGizmoMode::Scale,
                                "viewport-gizmo-scale",
                                "viewport-gizmo-scale-description",
                                localizer,
                            );
                        });
                    canvas.spawn((
                        ShapeGizmoValueLabel,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(12.0),
                            top: Val::Px(36.0),
                            padding: UiRect::axes(Val::Px(7.0), Val::Px(4.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL.with_alpha(0.94)),
                        BorderColor::all(theme::ACCENT_DIM),
                        Visibility::Hidden,
                        Pickable::IGNORE,
                    ));
                    canvas
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(8.0),
                                top: Val::Px(5.0),
                                height: Val::Px(28.0),
                                padding: UiRect::all(Val::Px(2.0)),
                                column_gap: Val::Px(2.0),
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(theme::PANEL.with_alpha(0.92)),
                            BorderColor::all(theme::BORDER),
                        ))
                        .with_children(|tools| {
                            spawn_viewport_tool_button(
                                tools,
                                EditorAction::FramePreview,
                                "viewport-frame-effect",
                                "viewport-frame-effect-description",
                                ViewportToolIcon::Frame,
                                localizer,
                            );
                            tools.spawn((
                                Node {
                                    width: Val::Px(1.0),
                                    height: Val::Px(14.0),
                                    margin: UiRect::horizontal(Val::Px(2.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::BORDER_BRIGHT),
                                Pickable::IGNORE,
                            ));
                            spawn_viewport_tool_button(
                                tools,
                                EditorAction::SetPreviewDisplayMode(PreviewDisplayMode::Wireframe),
                                "viewport-wireframe",
                                "viewport-wireframe-description",
                                ViewportToolIcon::Wireframe,
                                localizer,
                            );
                            spawn_viewport_tool_button(
                                tools,
                                EditorAction::SetPreviewDisplayMode(PreviewDisplayMode::Rendered),
                                "viewport-rendered",
                                "viewport-rendered-description",
                                ViewportToolIcon::Rendered,
                                localizer,
                            );
                        });
                });
            column.spawn((
                Text::new("0 LIVE PARTICLES  |  60 FPS"),
                ParticleCountLabel,
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn update_viewport_status_label(
    session: Res<EditorSession>,
    preview_runtime: Query<Ref<EffectRuntimeStatus>, With<PreviewEffectPlayer>>,
    mut labels: Query<&mut Text, With<ParticleCountLabel>>,
) {
    let runtime_changed = preview_runtime
        .iter()
        .any(|runtime| runtime.is_added() || runtime.is_changed());
    if !session.is_changed() && !runtime_changed {
        return;
    }
    let backend = preview_runtime
        .iter()
        .next()
        .map_or("DETECTING GPU", |runtime| match runtime.active {
            ActiveBackend::Pending => "DETECTING GPU",
            ActiveBackend::Gpu => "NATIVE GPU",
            ActiveBackend::GpuReadback => "GPU READBACK",
            ActiveBackend::CpuReference => "CPU FALLBACK",
        });
    for mut text in &mut labels {
        text.0 = format!("{} LIVE PARTICLES  |  {backend}", session.samples.len());
    }
}

#[derive(Clone, Copy)]
enum ViewportToolIcon {
    Frame,
    Wireframe,
    Rendered,
}

fn spawn_transform_gizmo_tool_button(
    parent: &mut ChildSpawnerCommands,
    mode: TransformGizmoMode,
    label_id: &'static str,
    description_id: &'static str,
    localizer: &Localizer,
) {
    let label = localizer.text(label_id);
    let shortcut = match mode {
        TransformGizmoMode::Translate => "1",
        TransformGizmoMode::Rotate => "2",
        TransformGizmoMode::Scale => "3",
    };
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            EditorAction::SetTransformGizmoMode(mode),
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::titled(label, localizer.text(description_id)).with_shortcut(shortcut),
            Node {
                width: Val::Px(22.0),
                min_width: Val::Px(22.0),
                height: Val::Px(22.0),
                padding: UiRect::ZERO,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| match mode {
            TransformGizmoMode::Translate => {
                button
                    .spawn((
                        Node {
                            width: Val::Px(12.0),
                            height: Val::Px(12.0),
                            position_type: PositionType::Relative,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|icon| {
                        icon.spawn((
                            TransformGizmoModeFill(mode),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(5.5),
                                top: Val::Px(1.0),
                                width: Val::Px(1.0),
                                height: Val::Px(10.0),
                                ..default()
                            },
                            BackgroundColor(theme::TEXT_MUTED),
                            Pickable::IGNORE,
                        ));
                        icon.spawn((
                            TransformGizmoModeFill(mode),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(1.0),
                                top: Val::Px(5.5),
                                width: Val::Px(10.0),
                                height: Val::Px(1.0),
                                ..default()
                            },
                            BackgroundColor(theme::TEXT_MUTED),
                            Pickable::IGNORE,
                        ));
                    });
            }
            TransformGizmoMode::Rotate => {
                button.spawn((
                    TransformGizmoModeOutline(mode),
                    Node {
                        width: Val::Px(12.0),
                        height: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BorderColor::all(theme::TEXT_MUTED),
                    Pickable::IGNORE,
                ));
            }
            TransformGizmoMode::Scale => {
                button
                    .spawn((
                        TransformGizmoModeOutline(mode),
                        Node {
                            width: Val::Px(11.0),
                            height: Val::Px(11.0),
                            position_type: PositionType::Relative,
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::all(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ))
                    .with_children(|cube| {
                        cube.spawn((
                            TransformGizmoModeFill(mode),
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(-2.0),
                                top: Val::Px(-2.0),
                                width: Val::Px(4.0),
                                height: Val::Px(4.0),
                                ..default()
                            },
                            BackgroundColor(theme::TEXT_MUTED),
                            Pickable::IGNORE,
                        ));
                    });
            }
        });
}

fn spawn_viewport_tool_button(
    parent: &mut ChildSpawnerCommands,
    action: EditorAction,
    label_id: &'static str,
    description_id: &'static str,
    icon: ViewportToolIcon,
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(localizer.text(label_id)),
            EditorTooltip::description(localizer.text(description_id)),
            Node {
                width: Val::Px(22.0),
                min_width: Val::Px(22.0),
                height: Val::Px(22.0),
                padding: UiRect::ZERO,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| match icon {
            ViewportToolIcon::Frame => {
                button
                    .spawn((
                        Node {
                            width: Val::Px(12.0),
                            height: Val::Px(12.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BorderColor::all(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ))
                    .with_children(|frame| {
                        frame.spawn((
                            Node {
                                width: Val::Px(3.0),
                                height: Val::Px(3.0),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(theme::TEXT_MUTED),
                            Pickable::IGNORE,
                        ));
                    });
            }
            ViewportToolIcon::Wireframe | ViewportToolIcon::Rendered => {
                let mode = if matches!(icon, ViewportToolIcon::Wireframe) {
                    PreviewDisplayMode::Wireframe
                } else {
                    PreviewDisplayMode::Rendered
                };
                button.spawn((
                    PreviewDisplayModeIcon(mode),
                    Node {
                        width: Val::Px(12.0),
                        height: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(theme::TEXT_MUTED),
                    Pickable::IGNORE,
                ));
            }
        });
}

#[derive(Clone, Copy, Debug)]
struct SelectedShapeModule {
    emitter: EmitterId,
    module: ModuleId,
    shape: EmitterShape,
}

fn selected_shape_module(session: &EditorSession) -> Option<SelectedShapeModule> {
    if session.pending_change.is_some() {
        return None;
    }
    let SemanticTarget::Module(module_id) = session.selection.primary else {
        return None;
    };
    let emitter = session.selected_layer();
    let module = emitter
        .modules
        .iter()
        .find(|module| module.id == module_id)?;
    match module_parameter(module, "shape") {
        Some(Value::Shape(shape)) if module.enabled => Some(SelectedShapeModule {
            emitter: emitter.id,
            module: module_id,
            shape,
        }),
        _ => None,
    }
}

fn sync_rendered_preview(
    mut commands: Commands,
    session: Res<EditorSession>,
    mut players: Query<(Entity, &mut EffectPlayer), With<PreviewEffectPlayer>>,
) {
    let desired = session
        .preview
        .as_ref()
        .map(|preview| preview.effect().clone());
    let Some(desired) = desired else {
        for (entity, _) in &mut players {
            commands.entity(entity).despawn();
        }
        return;
    };

    let Some((entity, mut player)) = players.iter_mut().next() else {
        spawn_preview_effect_player(&mut commands, &session, Transform::IDENTITY);
        return;
    };
    if !std::sync::Arc::ptr_eq(player.effect(), &desired) {
        if compiled_effects_differ_only_by_emitter_transforms(player.effect(), &desired) {
            if let Some(replacement) = configured_preview_player(&session) {
                *player = replacement;
            }
        } else {
            commands.entity(entity).despawn();
            spawn_preview_effect_player(&mut commands, &session, Transform::IDENTITY);
            return;
        }
    }

    player.playing = false;
    player.speed = session.speed;
    if player.instance.seed() != session.preview_seed {
        player.set_seed(session.preview_seed);
    }
    if player.frame() != session.frame() {
        player.seek_frame(session.frame());
    }
}

fn compiled_effects_differ_only_by_emitter_transforms(
    current: &CompiledEffect,
    desired: &CompiledEffect,
) -> bool {
    if current.emitters.len() != desired.emitters.len() {
        return false;
    }
    let mut normalized = current.clone();
    for (emitter, desired_emitter) in normalized.emitters.iter_mut().zip(&desired.emitters) {
        emitter.transform = desired_emitter.transform;
    }
    &normalized == desired
}

fn update_preview(mut session: ResMut<EditorSession>, mut profiler: ResMut<ProfilerState>) {
    let compiled = session
        .preview
        .as_ref()
        .map(|preview| preview.effect().clone());
    let mut samples = std::mem::take(&mut session.samples);
    let started = Instant::now();
    session.evaluate_preview(&mut samples);
    let elapsed = started.elapsed();
    session.samples = samples;
    if let Some(compiled) = compiled
        && profiler
            .ingest(ProfilerFrameSample::new(
                &compiled,
                &session.samples,
                elapsed,
            ))
            .profile_rebuilt()
    {
        session.ui_revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EFFECT_PATH, EFFECT_SOURCE};

    #[test]
    fn preview_cameras_are_disabled_without_an_active_viewport_canvas() {
        let mut app = App::new();
        app.add_systems(Update, sync_preview_camera_viewport);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let preview = app
            .world_mut()
            .spawn((PreviewRenderCamera, Camera::default()))
            .id();
        let overlay = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Camera::default(),
                RenderLayers::layer(15),
            ))
            .id();

        app.update();

        assert!(!app.world().get::<Camera>(preview).unwrap().is_active);
        assert!(!app.world().get::<Camera>(overlay).unwrap().is_active);
    }

    #[test]
    fn transform_gizmo_overlay_preserves_the_preview_color_buffer() {
        let mut app = App::new();
        app.add_systems(Update, configure_transform_gizmo_overlay_camera);
        let overlay = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Camera::default(),
                RenderLayers::layer(15),
            ))
            .id();
        let preview = app
            .world_mut()
            .spawn((
                PreviewRenderCamera,
                Camera3d::default(),
                Camera::default(),
                RenderLayers::layer(0),
            ))
            .id();

        app.update();

        assert!(matches!(
            app.world().get::<Camera>(overlay).unwrap().clear_color,
            ClearColorConfig::None
        ));
        assert_eq!(app.world().get::<Camera>(overlay).unwrap().order, 1);
        assert!(app.world().get::<Camera>(overlay).unwrap().is_active);
        assert!(!matches!(
            app.world().get::<Camera>(preview).unwrap().clear_color,
            ClearColorConfig::None
        ));
    }

    #[test]
    fn transform_gizmo_materials_render_after_transparent_particles() {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, configure_transform_gizmo_overlay_materials);
        let overlay_material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let scene_material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let overlay_root = app.world_mut().spawn_empty().id();
        let scene_root = app.world_mut().spawn_empty().id();
        app.world_mut().spawn((
            RenderLayers::layer(15),
            MeshMaterial3d(overlay_material.clone()),
            ChildOf(overlay_root),
        ));
        app.world_mut().spawn((
            RenderLayers::layer(0),
            MeshMaterial3d(scene_material.clone()),
            ChildOf(scene_root),
        ));

        app.update();

        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let overlay = materials.get(&overlay_material).unwrap();
        assert_eq!(overlay.alpha_mode, AlphaMode::Blend);
        assert_eq!(overlay.depth_bias, 10_000.0);
        let scene = materials.get(&scene_material).unwrap();
        assert_eq!(scene.alpha_mode, AlphaMode::Opaque);
        assert_eq!(scene.depth_bias, 0.0);
        assert!(
            app.world()
                .entity(overlay_root)
                .contains::<TransformGizmoVisualRoot>()
        );
        assert!(
            !app.world()
                .entity(scene_root)
                .contains::<TransformGizmoVisualRoot>()
        );
    }

    #[test]
    fn transform_gizmo_scale_feedback_stretches_only_the_dragged_axis() {
        let original = Transform::from_scale(Vec3::new(2.0, 3.0, 4.0));
        let current = Transform::from_scale(Vec3::new(5.0, 3.0, 4.0));

        let visual = transform_gizmo_drag_visual(
            TransformGizmoMode::Scale,
            TransformGizmoSpace::World,
            Some(TransformGizmoAxis::X),
            original,
            current,
        );

        assert_eq!(visual.scale, Vec3::new(2.5, 1.0, 1.0));
        assert!(visual.rotation.is_none());
    }

    #[test]
    fn transform_gizmo_rotation_feedback_uses_the_live_drag_delta() {
        let original = Transform::from_rotation(Quat::from_rotation_z(30.0_f32.to_radians()));
        let current = Transform::from_rotation(Quat::from_rotation_z(75.0_f32.to_radians()));

        let visual = transform_gizmo_drag_visual(
            TransformGizmoMode::Rotate,
            TransformGizmoSpace::World,
            Some(TransformGizmoAxis::Z),
            original,
            current,
        );

        let rotation = visual.rotation.unwrap();
        assert!(rotation.angle_between(Quat::from_rotation_z(45.0_f32.to_radians())) < 0.0001);
        assert_eq!(visual.scale, Vec3::ONE);
    }

    #[test]
    fn adaptive_grid_spacing_uses_stable_editor_scale_steps() {
        assert!((adaptive_grid_spacing(0.04) - 0.05).abs() < 0.000_001);
        assert_eq!(adaptive_grid_spacing(0.7), 1.0);
        assert_eq!(adaptive_grid_spacing(14.0), 20.0);
        assert_eq!(adaptive_grid_spacing(260.0), 500.0);
        assert_eq!(adaptive_grid_spacing(f32::NAN), 1.0);
    }

    #[test]
    fn framing_preview_restores_default_camera_around_effect() {
        let mut controller = PreviewCameraController {
            focus: Vec3::new(12.0, -7.0, 3.0),
            distance: 9.0,
            yaw: 1.2,
            pitch: -0.8,
            frame_requested: true,
        };
        let effect_position = Vec3::new(24.0, 6.0, -2.0);

        controller.frame_effect(effect_position);

        assert_eq!(controller.focus, effect_position);
        assert_eq!(controller.distance, 140.0);
        assert_eq!(controller.yaw, 0.0);
        assert_eq!(controller.pitch, DEFAULT_PREVIEW_PITCH);
        assert!(!controller.frame_requested);
    }

    #[test]
    fn default_preview_camera_views_the_ground_grid_from_above() {
        let camera = preview_camera_transform(Vec3::ZERO, 140.0, 0.0, DEFAULT_PREVIEW_PITCH);

        assert!(camera.translation.y > 0.0);
        assert!((camera.forward().dot(Vec3::NEG_Y)).abs() > 0.25);
    }

    #[test]
    fn shape_gizmo_drag_preserves_unedited_cone_component() {
        let cone = EmitterShape::Cone {
            radius: 12.0,
            depth: 24.0,
        };

        assert_eq!(
            shape_after_gizmo_drag(cone, ShapeGizmoHandle::Radius, 18.0),
            EmitterShape::Cone {
                radius: 18.0,
                depth: 24.0,
            }
        );
        assert_eq!(
            shape_after_gizmo_drag(cone, ShapeGizmoHandle::Depth, 31.0),
            EmitterShape::Cone {
                radius: 12.0,
                depth: 31.0,
            }
        );
    }

    #[test]
    fn shape_gizmo_drag_clamps_degenerate_values() {
        assert_eq!(
            shape_after_gizmo_drag(
                EmitterShape::Circle { radius: 12.0 },
                ShapeGizmoHandle::Radius,
                0.0,
            ),
            EmitterShape::Circle { radius: 0.1 }
        );
        assert_eq!(
            shape_after_gizmo_drag(
                EmitterShape::Cone {
                    radius: 12.0,
                    depth: 24.0,
                },
                ShapeGizmoHandle::Depth,
                0.0,
            ),
            EmitterShape::Cone {
                radius: 12.0,
                depth: 0.1,
            }
        );
    }

    #[test]
    fn shape_gizmo_edits_volumetric_dimensions_independently() {
        let box_shape = EmitterShape::Box {
            half_extents: [2.0, 3.0, 4.0],
        };
        assert_eq!(
            shape_after_gizmo_drag(box_shape, ShapeGizmoHandle::ExtentZ, 9.0),
            EmitterShape::Box {
                half_extents: [2.0, 3.0, 9.0],
            }
        );
        assert_eq!(
            shape_after_gizmo_drag(
                EmitterShape::Cylinder {
                    radius: 5.0,
                    depth: 8.0,
                },
                ShapeGizmoHandle::Depth,
                12.0,
            ),
            EmitterShape::Cylinder {
                radius: 5.0,
                depth: 12.0,
            }
        );
    }

    #[test]
    fn selecting_shape_module_keeps_root_transform_gizmo_available() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let shape_module = session
            .selected_layer()
            .module_by_type(aestra_bevy::MODULE_SHAPE)
            .unwrap()
            .id;
        session.selection.primary = SemanticTarget::Module(shape_module);
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<ShapeGizmoState>()
            .add_systems(Update, sync_transform_gizmo_focus);
        let player = app.world_mut().spawn(EmitterTransformGizmoProxy).id();

        app.update();

        assert!(app.world().entity(player).contains::<TransformGizmoFocus>());
    }

    #[test]
    fn emitter_gizmo_previews_then_commits_one_undoable_transform() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let gizmo = TransformGizmoState {
            active: true,
            ..default()
        };
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(gizmo)
            .init_resource::<EmitterTransformGizmoInteraction>()
            .add_systems(Update, update_emitter_transform_gizmo);
        app.world_mut().spawn((
            EmitterTransformGizmoProxy,
            Transform::from_xyz(4.0, 5.0, 6.0),
        ));

        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(
            session.selected_layer().transform,
            EmitterTransform::default()
        );
        assert_eq!(
            session.preview.as_ref().unwrap().effect().emitters[0]
                .transform
                .translation,
            [4.0, 5.0, 6.0],
            "drag preview must update the compiled effect before release"
        );

        app.world_mut().resource_mut::<TransformGizmoState>().active = false;
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(
            session.selected_layer().transform.translation,
            [4.0, 5.0, 6.0]
        );

        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selected_layer()
                .transform,
            EmitterTransform::default()
        );
    }

    #[test]
    fn editor_preview_player_uses_the_compiled_effect_timeline_and_seed() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.preview_seed = 42;
        session.clock.seek_frame(37, session.playback_duration());

        let player = configured_preview_player(&session).unwrap();

        assert!(std::sync::Arc::ptr_eq(
            player.effect(),
            session.preview.as_ref().unwrap().effect()
        ));
        assert_eq!(player.frame(), session.frame());
        assert_eq!(player.instance.seed(), 42);
        assert!(!player.playing);
    }

    #[test]
    fn transform_only_preview_updates_the_existing_player_in_place() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let player = configured_preview_player(&session).unwrap();
        let emitter = session.selected_layer().id;
        let transform = EmitterTransform {
            translation: [7.0, -3.0, 2.0],
            ..default()
        };
        assert!(session.preview_interaction(EffectTransaction::single(
            "Preview emitter transform",
            EffectCommand::SetEmitterTransform {
                id: emitter,
                transform,
            },
        )));

        let mut app = App::new();
        app.insert_resource(session)
            .add_systems(Update, sync_rendered_preview);
        let player_entity = app.world_mut().spawn((PreviewEffectPlayer, player)).id();

        app.update();

        let world = app.world();
        let player = world.get::<EffectPlayer>(player_entity).unwrap();
        assert_eq!(player.effect().emitters[0].transform, transform);
        assert!(std::sync::Arc::ptr_eq(
            player.effect(),
            world
                .resource::<EditorSession>()
                .preview
                .as_ref()
                .unwrap()
                .effect()
        ));
    }

    #[test]
    fn live_player_replacement_rejects_structural_compiler_changes() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let current = session.preview.as_ref().unwrap().effect();
        let mut transformed = current.as_ref().clone();
        transformed.emitters[0].transform.translation[0] = 4.0;
        assert!(compiled_effects_differ_only_by_emitter_transforms(
            current,
            &transformed
        ));

        transformed.emitters[0].duration += 0.25;
        assert!(!compiled_effects_differ_only_by_emitter_transforms(
            current,
            &transformed
        ));
    }
}
