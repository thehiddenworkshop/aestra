//! Advanced compiler-artifact inspection and semantic source navigation.

use crate::feathers::panel::{
    spawn_panel_empty_state, spawn_panel_label_value, spawn_panel_muted_line, spawn_panel_section,
};
use crate::*;
use aestra_runtime::{CompiledEffect, CompiledEmitter, Instruction, RuntimeStage};

pub(crate) struct EditorCompilerInspectorPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CompilerInspectorSet {
    Actions,
}

impl Plugin for EditorCompilerInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            handle_compiler_inspector_actions.in_set(CompilerInspectorSet::Actions),
        );
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum CompilerInspectorAction {
    SelectTarget(SemanticTarget),
}

fn handle_compiler_inspector_actions(
    mut actions: Query<
        (&Interaction, &CompilerInspectorAction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut session: ResMut<EditorSession>,
    mut workspace: ResMut<CurvesState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut inspector_focus: ResMut<InspectorFocus>,
    localizer: Res<Localizer>,
) {
    for (interaction, action, mut background) in &mut actions {
        let CompilerInspectorAction::SelectTarget(target) = *action;
        match *interaction {
            Interaction::Hovered => background.0 = theme::BUTTON_HOVER,
            Interaction::None => {
                background.0 = if target == session.selection.primary {
                    theme::SELECTION
                } else {
                    theme::PANEL
                };
            }
            Interaction::Pressed => {
                background.0 = theme::ACCENT_DIM;
                workspace.clear();
                if focus_compiled_target(&mut session, &mut inspector_focus, target, &localizer) {
                    reveal_dock_panel(&mut layout, &mut session, DockPanel::Inspector);
                } else {
                    session.status = localizer.text("compiler-status-pending-target");
                    session.ui_revision += 1;
                    reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                }
            }
        }
    }
}

pub(crate) fn spawn_compiler_inspector_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let compiled = session
        .preview
        .as_ref()
        .map(|preview| preview.effect().as_ref());
    let (state_label, state_color) = compiler_inspector_status(session, compiled.is_some());

    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(9.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new(localizer.text("generated-compiled-plan")),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(6.0),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(state_color),
                    ));
                    header.spawn((
                        Text::new(localizer.text(state_label)),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(state_color),
                    ));
                });

            let Some(compiled) = compiled else {
                spawn_panel_empty_state(
                    panel,
                    &localizer.text("generated-no-artifact"),
                    &localizer.text("generated-no-artifact-description"),
                    Color::srgb(1.0, 0.38, 0.32),
                );
                return;
            };

            spawn_compiled_summary(panel, compiled, localizer);
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::CompilerInspector,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.0)),
                            row_gap: Val::Px(10.0),
                            ..default()
                        },
                        |content| {
                            spawn_compiled_layout(content, compiled, localizer);
                            spawn_compiled_parameters(content, compiled, session, localizer);
                            for (emitter_index, emitter) in compiled.emitters.iter().enumerate() {
                                spawn_compiled_emitter(
                                    content,
                                    compiled,
                                    emitter,
                                    emitter_index,
                                    session,
                                    localizer,
                                );
                            }
                            spawn_wesl_backend(content, localizer);
                        },
                    );
                });
        });
}

fn compiler_inspector_status(session: &EditorSession, has_artifact: bool) -> (&'static str, Color) {
    if session
        .pending_change
        .as_ref()
        .is_some_and(|pending| !pending.can_apply)
        && has_artifact
    {
        ("generated-status-last-valid", Color::srgb(1.0, 0.74, 0.30))
    } else if session.pending_change.is_some() && has_artifact {
        ("generated-status-pending", theme::ACCENT)
    } else if has_artifact {
        ("generated-status-live", Color::srgb(0.35, 0.88, 0.57))
    } else {
        ("generated-status-unavailable", Color::srgb(1.0, 0.38, 0.32))
    }
}

fn spawn_compiled_summary(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    localizer: &Localizer,
) {
    let instruction_count = compiled
        .emitters
        .iter()
        .map(|emitter| {
            emitter.execution.emitter_update.len()
                + emitter.execution.particle_spawn.len()
                + emitter.execution.particle_update.len()
        })
        .sum::<usize>();
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(42.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(18.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|summary| {
            spawn_compiled_metric(
                summary,
                &localizer.text("generated-emitters"),
                compiled.emitters.len(),
            );
            spawn_compiled_metric(summary, &localizer.text("generated-ops"), instruction_count);
            spawn_compiled_metric(
                summary,
                &localizer.text("generated-attributes"),
                compiled.particle_layout.attributes.len(),
            );
            spawn_compiled_metric(
                summary,
                &localizer.text("generated-parameters"),
                compiled.parameters.len(),
            );
            spawn_compiled_metric(
                summary,
                &localizer.text("generated-capacity"),
                compiled.max_particles,
            );
            summary.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            summary.spawn((
                Text::new(format!(
                    "{}  ·  {:.2}s  ·  {:?}",
                    compiled.name.to_uppercase(),
                    compiled.duration,
                    compiled.seek_mode
                )),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn spawn_compiled_metric(parent: &mut ChildSpawnerCommands, label: &str, value: usize) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            ..default()
        })
        .with_children(|metric| {
            metric.spawn((
                Text::new(value.to_string()),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::TEXT),
            ));
            metric.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn spawn_compiled_layout(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    localizer: &Localizer,
) {
    spawn_panel_section(
        parent,
        &localizer.text("generated-particle-layout"),
        |section| {
            spawn_panel_label_value(
                section,
                &localizer.text("generated-stored"),
                &format_particle_attributes(&compiled.particle_layout.attributes),
            );
            spawn_panel_label_value(
                section,
                &localizer.text("generated-transient"),
                &format_particle_attributes(&compiled.particle_layout.transient_attributes),
            );
            spawn_panel_label_value(section, &localizer.text("generated-optimized"), &{
                let mut args = FluentArgs::new();
                args.set("constants", compiled.optimizations.constant_expressions);
                args.set("reads", compiled.optimizations.runtime_parameter_reads);
                args.set("removed", compiled.optimizations.eliminated_attributes);
                localizer.text_with("generated-optimization-summary", &args)
            });
        },
    );
}

fn spawn_compiled_parameters(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    session: &EditorSession,
    localizer: &Localizer,
) {
    spawn_panel_section(
        parent,
        &localizer.text("generated-parameter-table"),
        |section| {
            if compiled.parameters.is_empty() {
                spawn_panel_muted_line(section, &localizer.text("generated-no-runtime-parameters"));
                return;
            }
            for (index, parameter) in compiled.parameters.iter().enumerate() {
                spawn_compiled_target_row(
                    section,
                    SemanticTarget::Parameter(parameter.source),
                    session.selection.primary == SemanticTarget::Parameter(parameter.source),
                    &format!("P{index:03}"),
                    &parameter.name,
                    &format!("{:?}  ·  {:?}", parameter.value_type, parameter.default),
                );
            }
        },
    );
}

fn spawn_compiled_emitter(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    emitter: &CompiledEmitter,
    emitter_index: usize,
    session: &EditorSession,
    localizer: &Localizer,
) {
    spawn_panel_section(
        parent,
        &format!(
            "EMITTER {emitter_index:02}  ·  {}",
            emitter.name.to_uppercase()
        ),
        |section| {
            let enabled = localizer.text(if emitter.enabled {
                "generated-enabled"
            } else {
                "generated-disabled"
            });
            spawn_compiled_target_row(
                section,
                SemanticTarget::Emitter(emitter.source),
                session.selection.primary == SemanticTarget::Emitter(emitter.source),
                &format!("E{emitter_index:02}"),
                &enabled,
                &format!(
                    "start {:.2}s  ·  duration {:.2}s  ·  position {:?}  ·  scale {:?}  ·  capacity {}  ·  {}",
                    emitter.start_time,
                    emitter.duration,
                    emitter.transform.translation,
                    emitter.transform.scale,
                    emitter.max_particles,
                    emitter.source
                ),
            );
            spawn_compiled_stage(
                section,
                compiled,
                emitter,
                emitter_index,
                RuntimeStage::EmitterUpdate,
                &emitter.execution.emitter_update,
                session,
                localizer,
            );
            spawn_compiled_stage(
                section,
                compiled,
                emitter,
                emitter_index,
                RuntimeStage::ParticleSpawn,
                &emitter.execution.particle_spawn,
                session,
                localizer,
            );
            spawn_compiled_stage(
                section,
                compiled,
                emitter,
                emitter_index,
                RuntimeStage::ParticleUpdate,
                &emitter.execution.particle_update,
                session,
                localizer,
            );
            if emitter.renderers.is_empty() {
                spawn_compiled_stage_heading(
                    section,
                    &localizer.text("generated-renderers"),
                    0,
                    localizer,
                );
            } else {
                spawn_compiled_stage_heading(
                    section,
                    &localizer.text("generated-renderers"),
                    emitter.renderers.len(),
                    localizer,
                );
                for (index, renderer) in emitter.renderers.iter().enumerate() {
                    let material = compiled
                        .material(renderer.material)
                        .expect("compiled renderer material must exist");
                    let texture = material
                        .texture
                        .and_then(|id| compiled.assets.iter().find(|asset| asset.source == id))
                        .map_or("procedural".to_string(), |asset| {
                            format!("texture {}", asset.name)
                        });
                    spawn_compiled_target_row(
                        section,
                        SemanticTarget::Renderer(renderer.source),
                        session.selection.primary == SemanticTarget::Renderer(renderer.source),
                        &format!("R{index:03}"),
                        &localizer.text("generated-sprite-draw"),
                        &format!(
                            "material {}  ·  {:?} blend  ·  softness {:?}  ·  {texture}  ·  {}",
                            material.name, material.blend, material.softness, renderer.source,
                        ),
                    );
                }
            }
        },
    );
}

fn spawn_compiled_stage(
    parent: &mut ChildSpawnerCommands,
    compiled: &CompiledEffect,
    _emitter: &CompiledEmitter,
    emitter_index: usize,
    stage: RuntimeStage,
    instructions: &[Instruction],
    session: &EditorSession,
    localizer: &Localizer,
) {
    spawn_compiled_stage_heading(
        parent,
        &localizer.text(runtime_stage_message_id(stage)),
        instructions.len(),
        localizer,
    );
    for (instruction_index, instruction) in instructions.iter().enumerate() {
        let module = instruction.source();
        let source_name = authored_module_name(compiled_source_effect(session), module);
        let location =
            compiled
                .source_map
                .get(&module)
                .copied()
                .unwrap_or(aestra_runtime::IrLocation {
                    emitter_index,
                    stage,
                    instruction_index,
                });
        spawn_compiled_target_row(
            parent,
            SemanticTarget::Module(module),
            session.selection.primary == SemanticTarget::Module(module),
            &format!(
                "E{:02}/{}{:03}",
                location.emitter_index,
                runtime_stage_code(location.stage),
                location.instruction_index
            ),
            &format!("{}  ·  {source_name}", instruction_opcode(instruction)),
            &instruction_summary(instruction),
        );
    }
}

fn spawn_compiled_stage_heading(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    count: usize,
    localizer: &Localizer,
) {
    parent.spawn((
        Text::new(format!(
            "{title}  ·  {count} {}",
            localizer.text("generated-ops")
        )),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            margin: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(8.0), Val::Px(2.0)),
            ..default()
        },
    ));
}

fn spawn_wesl_backend(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    spawn_panel_section(
        parent,
        &localizer.text("generated-wesl-backend"),
        |section| {
            spawn_panel_label_value(
                section,
                &localizer.text("generated-simulation"),
                "aestra_simulation.wesl  ·  reset @compute(1)  ·  simulate @compute(64)",
            );
            spawn_panel_label_value(
                section,
                &localizer.text("assets-sprite"),
                "aestra_sprite_render.wesl  ·  vertex  ·  fragment_alpha  ·  fragment_additive",
            );
            spawn_panel_muted_line(section, &localizer.text("generated-wesl-description"));
        },
    );
}

fn spawn_compiled_target_row(
    parent: &mut ChildSpawnerCommands,
    target: SemanticTarget,
    selected: bool,
    address: &str,
    opcode: &str,
    detail: &str,
) {
    parent
        .spawn((
            Button,
            EditorNativeControl,
            CompilerInspectorAction::SelectTarget(target),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(31.0),
                padding: UiRect::axes(Val::Px(7.0), Val::Px(5.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(9.0),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::PANEL
            }),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(address),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
                Node {
                    width: Val::Px(80.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            row.spawn((
                Text::new(opcode),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Node {
                    width: Val::Px(190.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            row.spawn((
                Text::new(detail),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}

fn format_particle_attributes(attributes: &[aestra_runtime::ParticleAttribute]) -> String {
    if attributes.is_empty() {
        return "none".into();
    }
    attributes
        .iter()
        .map(|attribute| format!("{attribute:?}").to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("  ·  ")
}

fn authored_module_name(effect: &EffectAsset, module: ModuleId) -> String {
    effect
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .find(|candidate| candidate.id == module)
        .map(|module| module.module_type.0.to_ascii_uppercase())
        .unwrap_or_else(|| "UNKNOWN MODULE".into())
}

fn compiled_source_effect(session: &EditorSession) -> &EffectAsset {
    session
        .pending_change
        .as_ref()
        .filter(|pending| pending.can_apply)
        .map(|pending| pending.preview.candidate())
        .unwrap_or(&session.effect)
}

fn runtime_stage_message_id(stage: RuntimeStage) -> &'static str {
    match stage {
        RuntimeStage::EmitterUpdate => "generated-stage-emitter-update",
        RuntimeStage::ParticleSpawn => "generated-stage-particle-spawn",
        RuntimeStage::ParticleUpdate => "generated-stage-particle-update",
    }
}

fn runtime_stage_code(stage: RuntimeStage) -> &'static str {
    match stage {
        RuntimeStage::EmitterUpdate => "EU",
        RuntimeStage::ParticleSpawn => "PS",
        RuntimeStage::ParticleUpdate => "PU",
    }
}

fn instruction_opcode(instruction: &Instruction) -> &'static str {
    match instruction {
        Instruction::Emit { .. } => "EMIT",
        Instruction::SampleShape { .. } => "SAMPLE SHAPE",
        Instruction::Initialize { .. } => "INITIALIZE",
        Instruction::Motion { .. } => "MOTION",
        Instruction::Appearance { .. } => "APPEARANCE",
    }
}

fn instruction_summary(instruction: &Instruction) -> String {
    match instruction {
        Instruction::Emit {
            spawn_rate,
            burst_count,
            ..
        } => format!("rate {spawn_rate:?}  ·  burst {burst_count:?}"),
        Instruction::SampleShape { shape, .. } => format!("shape {shape:?}"),
        Instruction::Initialize {
            lifetime,
            speed,
            direction,
            spread_degrees,
            angular_velocity,
            ..
        } => format!(
            "life {lifetime:?}  ·  speed {speed:?}  ·  direction {direction:?}  ·  spread {spread_degrees:?}  ·  angular {angular_velocity:?}"
        ),
        Instruction::Motion {
            gravity,
            drag,
            turbulence,
            ..
        } => format!("gravity {gravity:?}  ·  drag {drag:?}  ·  turbulence {turbulence:?}"),
        Instruction::Appearance {
            size,
            opacity,
            color,
            ..
        } => format!("size {size:?}  ·  opacity {opacity:?}  ·  color {color:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_inspector_uses_the_live_compiler_artifact() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let compiled = session.preview.as_ref().unwrap().effect();
        let instruction_count = compiled
            .emitters
            .iter()
            .flat_map(|emitter| {
                emitter
                    .execution
                    .emitter_update
                    .iter()
                    .chain(emitter.execution.particle_spawn.iter())
                    .chain(emitter.execution.particle_update.iter())
            })
            .count();

        assert_eq!(
            compiler_inspector_status(&session, true).0,
            "generated-status-live"
        );
        assert_eq!(compiled.emitters.len(), session.effect.emitters.len());
        assert_eq!(instruction_count, compiled.source_map.len());
        assert!(!compiled.particle_layout.attributes.is_empty());
        assert_eq!(
            instruction_opcode(&compiled.emitters[0].execution.emitter_update[0]),
            "EMIT"
        );
    }

    #[test]
    fn selecting_a_compiled_target_focuses_the_inspector() {
        let mut app = App::new();
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let target = SemanticTarget::Module(session.effect.emitters[0].modules[0].id);
        app.insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .init_resource::<InspectorFocus>()
            .add_systems(Update, handle_compiler_inspector_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            CompilerInspectorAction::SelectTarget(target),
            BackgroundColor(theme::PANEL),
        ));

        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.selection.primary, target);
        assert!(session.ui_revision > 0);
    }
}
