use aestra_authoring::{
    CommandError, CommandHistory, EffectCommand, EffectDiff, EffectTransaction, LockState,
    Selection,
};
use aestra_bevy::{
    AssetError, BlendMode, ColorKey, CurveKey, EffectAsset, Emitter, ModuleId, ModuleInstance,
    RendererId, RendererInstance, RendererProperties, ValidationReport, Value,
};
use aestra_compiler::{CompileError, EffectCompiler};
use aestra_runtime::EffectInstance;
use bevy::prelude::Resource;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum SessionError {
    #[error(transparent)]
    Asset(#[from] AssetError),
    #[error(transparent)]
    Compile(#[from] CompileError),
}

#[derive(Resource)]
pub(crate) struct EditorSession {
    pub effect: EffectAsset,
    pub source_path: Option<PathBuf>,
    pub selection: Selection,
    pub locks: LockState,
    pub diagnostics: ValidationReport,
    pub last_diff: EffectDiff,
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub dirty: bool,
    pub status: String,
    pub samples: Vec<aestra_bevy::ParticleSample>,
    pub preview: Option<EffectInstance>,
    pub ui_revision: u64,
    history: CommandHistory,
}

impl EditorSession {
    pub fn from_embedded_sample(source: &str, path: impl Into<PathBuf>) -> Self {
        let effect = EffectAsset::from_ron(source)
            .expect("the bundled Prism Bloom sample must always be valid");
        let selection = Selection::for_effect(&effect);
        let diagnostics = effect.validation_report();
        let preview =
            compile_preview(&effect).expect("the bundled Prism Bloom sample must always compile");
        Self {
            effect,
            source_path: Some(path.into()),
            selection,
            locks: LockState::default(),
            diagnostics,
            last_diff: EffectDiff::default(),
            time: 0.0,
            playing: true,
            speed: 1.0,
            dirty: false,
            status: "Previewing embedded Prism Bloom".into(),
            samples: Vec::with_capacity(384),
            preview: Some(preview),
            ui_revision: 0,
            history: CommandHistory::default(),
        }
    }

    pub fn restart(&mut self) {
        self.time = 0.0;
        if let Some(preview) = &mut self.preview {
            preview.restart();
        }
        self.playing = true;
        self.status = "Choreography restarted".into();
    }

    pub fn new_effect(&mut self) {
        self.effect = blank_effect();
        self.preview = Some(compile_preview(&self.effect).expect("blank effect must compile"));
        self.source_path = None;
        self.selection = Selection::for_effect(&self.effect);
        self.locks = LockState::default();
        self.diagnostics = self.effect.validation_report();
        self.last_diff = EffectDiff::default();
        self.time = 0.0;
        self.playing = false;
        self.dirty = true;
        self.history.clear();
        self.status = "Created an untitled effect".into();
        self.ui_revision += 1;
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let path = path.as_ref();
        let effect = EffectAsset::load_ron(path)?;
        let preview = compile_preview(&effect)?;
        self.effect = effect;
        self.preview = Some(preview);
        self.source_path = Some(path.to_owned());
        self.selection = Selection::for_effect(&self.effect);
        self.locks = LockState::default();
        self.diagnostics = self.effect.validation_report();
        self.last_diff = EffectDiff::default();
        self.time = 0.0;
        self.playing = false;
        self.dirty = false;
        self.history.clear();
        self.status = format!("Opened {}", path.display());
        self.ui_revision += 1;
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), AssetError> {
        let Some(path) = self.source_path.clone() else {
            return Ok(());
        };
        self.save_as(path)
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), AssetError> {
        let path = path.as_ref();
        self.effect.save_ron(path)?;
        self.source_path = Some(path.to_owned());
        self.dirty = false;
        self.status = format!("Saved {}", path.display());
        self.ui_revision += 1;
        Ok(())
    }

    pub fn execute(
        &mut self,
        label: impl Into<String>,
        command: EffectCommand,
        rebuild_ui: bool,
    ) -> bool {
        let label = label.into();
        self.execute_transaction(EffectTransaction::single(label, command), rebuild_ui)
    }

    pub fn execute_transaction(
        &mut self,
        transaction: EffectTransaction,
        rebuild_ui: bool,
    ) -> bool {
        let label = transaction.label.clone();
        match self
            .history
            .execute(&mut self.effect, &self.locks, transaction)
        {
            Ok(diff) => {
                self.last_diff = diff;
                self.refresh_preview();
                self.selection.repair(&self.effect);
                self.time = self.time.clamp(0.0, self.effect.duration);
                self.dirty = true;
                self.status = label;
                if rebuild_ui {
                    self.ui_revision += 1;
                }
                true
            }
            Err(error) => {
                self.record_command_error("Edit failed", error);
                false
            }
        }
    }

    pub fn undo(&mut self) {
        match self.history.undo(&mut self.effect) {
            Ok(Some(result)) => {
                self.selection.repair(&self.effect);
                self.refresh_preview();
                self.last_diff = result.diff;
                self.time = self.time.clamp(0.0, self.effect.duration);
                self.status = format!("Undid {}", result.label);
                self.dirty = true;
                self.ui_revision += 1;
            }
            Ok(None) => self.status = "Nothing to undo".into(),
            Err(error) => self.record_command_error("Undo failed", error),
        }
    }

    pub fn redo(&mut self) {
        match self.history.redo(&mut self.effect) {
            Ok(Some(result)) => {
                self.selection.repair(&self.effect);
                self.refresh_preview();
                self.last_diff = result.diff;
                self.time = self.time.clamp(0.0, self.effect.duration);
                self.status = format!("Redid {}", result.label);
                self.dirty = true;
                self.ui_revision += 1;
            }
            Ok(None) => self.status = "Nothing to redo".into(),
            Err(error) => self.record_command_error("Redo failed", error),
        }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn selected_layer_index(&self) -> usize {
        let id = self
            .selection
            .emitter(&self.effect)
            .expect("the editor always keeps an emitter selected");
        self.effect
            .emitters
            .iter()
            .position(|emitter| emitter.id == id)
            .expect("selected emitter must exist")
    }

    pub fn selected_layer(&self) -> &Emitter {
        &self.effect.emitters[self.selected_layer_index()]
    }

    pub fn select_layer(&mut self, index: usize) {
        let Some(emitter) = self.effect.emitters.get(index) else {
            self.status = format!("Emitter index {index} does not exist");
            return;
        };
        self.selection.select_emitter(emitter.id);
        self.status = format!("Selected {}", emitter.name);
        self.ui_revision += 1;
    }

    pub fn add_layer(&mut self) {
        let index = self.effect.emitters.len();
        let mut emitter = default_layer(index);
        emitter.duration = self.effect.duration;
        let id = emitter.id;
        if self.execute(
            "Added emitter layer",
            EffectCommand::AddEmitter { emitter, index },
            true,
        ) {
            self.selection.select_emitter(id);
        }
    }

    pub fn duplicate_selected_layer(&mut self) {
        let id = self.selected_layer().id;
        let Some(command) = EffectCommand::duplicate_emitter(&self.effect, id) else {
            self.status = "Selected emitter no longer exists".into();
            return;
        };
        let duplicate = match &command {
            EffectCommand::AddEmitter { emitter, .. } => emitter.id,
            _ => unreachable!(),
        };
        if self.execute("Duplicated emitter layer", command, true) {
            self.selection.select_emitter(duplicate);
        }
    }

    pub fn delete_selected_layer(&mut self) {
        if self.effect.emitters.len() <= 1 {
            self.status = "An effect must keep at least one layer".into();
            return;
        }
        let index = self.selected_layer_index();
        let id = self.effect.emitters[index].id;
        let next = self
            .effect
            .emitters
            .get(index + 1)
            .or_else(|| {
                index
                    .checked_sub(1)
                    .and_then(|index| self.effect.emitters.get(index))
            })
            .map(|emitter| emitter.id);
        if self.execute(
            "Deleted emitter layer",
            EffectCommand::RemoveEmitter { id },
            true,
        ) && let Some(next) = next
        {
            self.selection.select_emitter(next);
        }
    }

    pub fn add_module(&mut self, module: ModuleInstance) {
        let emitter = self.selected_layer();
        let emitter_id = emitter.id;
        let index = emitter
            .modules
            .iter()
            .rposition(|item| item.stage == module.stage)
            .map_or(emitter.modules.len(), |index| index + 1);
        let module_id = module.id;
        if self.execute(
            "Added module",
            EffectCommand::AddModule {
                emitter: emitter_id,
                module,
                index,
            },
            true,
        ) {
            self.selection.primary = aestra_authoring::SemanticTarget::Module(module_id);
        }
    }

    pub fn set_module_parameter(&mut self, module: ModuleId, parameter: &str, value: Value) {
        let emitter = self.selected_layer().id;
        self.execute(
            format!("Changed {parameter}"),
            EffectCommand::SetModuleParameter {
                emitter,
                module,
                parameter: parameter.into(),
                value,
            },
            true,
        );
    }

    pub fn add_curve_key(
        &mut self,
        module: ModuleId,
        parameter: &str,
        index: usize,
        key: CurveKey,
    ) {
        let emitter = self.selected_layer().id;
        self.execute(
            format!("Added {parameter} curve key"),
            EffectCommand::AddCurveKey {
                emitter,
                module,
                parameter: parameter.into(),
                key,
                index,
            },
            true,
        );
    }

    pub fn set_curve_key(
        &mut self,
        module: ModuleId,
        parameter: &str,
        index: usize,
        key: CurveKey,
    ) {
        let emitter = self.selected_layer().id;
        self.execute(
            format!("Changed {parameter} curve key"),
            EffectCommand::SetCurveKey {
                emitter,
                module,
                parameter: parameter.into(),
                index,
                key,
            },
            true,
        );
    }

    pub fn remove_curve_key(&mut self, module: ModuleId, parameter: &str, index: usize) {
        let emitter = self.selected_layer().id;
        self.execute(
            format!("Removed {parameter} curve key"),
            EffectCommand::RemoveCurveKey {
                emitter,
                module,
                parameter: parameter.into(),
                index,
            },
            true,
        );
    }

    pub fn add_gradient_key(
        &mut self,
        module: ModuleId,
        parameter: &str,
        index: usize,
        key: ColorKey,
    ) {
        let emitter = self.selected_layer().id;
        self.execute(
            format!("Added {parameter} gradient key"),
            EffectCommand::AddGradientKey {
                emitter,
                module,
                parameter: parameter.into(),
                key,
                index,
            },
            true,
        );
    }

    pub fn set_gradient_key(
        &mut self,
        module: ModuleId,
        parameter: &str,
        index: usize,
        key: ColorKey,
    ) {
        let emitter = self.selected_layer().id;
        self.execute(
            format!("Changed {parameter} gradient key"),
            EffectCommand::SetGradientKey {
                emitter,
                module,
                parameter: parameter.into(),
                index,
                key,
            },
            true,
        );
    }

    pub fn remove_gradient_key(&mut self, module: ModuleId, parameter: &str, index: usize) {
        let emitter = self.selected_layer().id;
        self.execute(
            format!("Removed {parameter} gradient key"),
            EffectCommand::RemoveGradientKey {
                emitter,
                module,
                parameter: parameter.into(),
                index,
            },
            true,
        );
    }

    pub fn toggle_module(&mut self, id: ModuleId) {
        let emitter = self.selected_layer();
        let Some(module) = emitter.modules.iter().find(|module| module.id == id) else {
            self.status = "Module no longer exists".into();
            return;
        };
        self.execute(
            "Toggled module",
            EffectCommand::SetModuleEnabled {
                emitter: emitter.id,
                module: id,
                enabled: !module.enabled,
            },
            true,
        );
    }

    pub fn move_module(&mut self, id: ModuleId, direction: i8) {
        let emitter = self.selected_layer();
        let Some(index) = emitter.modules.iter().position(|module| module.id == id) else {
            self.status = "Module no longer exists".into();
            return;
        };
        let stage = &emitter.modules[index].stage;
        let sibling = if direction < 0 {
            emitter.modules[..index]
                .iter()
                .rposition(|module| &module.stage == stage)
        } else {
            emitter.modules[index + 1..]
                .iter()
                .position(|module| &module.stage == stage)
                .map(|offset| index + offset + 1)
        };
        let Some(target) = sibling else {
            self.status = "Module is already at the edge of its stage".into();
            return;
        };
        self.execute(
            "Reordered module",
            EffectCommand::MoveModule {
                emitter: emitter.id,
                module: id,
                index: target,
            },
            true,
        );
    }

    pub fn duplicate_module(&mut self, id: ModuleId) {
        let emitter = self.selected_layer().id;
        let Some(command) = EffectCommand::duplicate_module(&self.effect, emitter, id) else {
            self.status = "Module no longer exists".into();
            return;
        };
        let duplicate = match &command {
            EffectCommand::AddModule { module, .. } => module.id,
            _ => unreachable!(),
        };
        if self.execute("Duplicated module", command, true) {
            self.selection.primary = aestra_authoring::SemanticTarget::Module(duplicate);
        }
    }

    pub fn delete_module(&mut self, id: ModuleId) {
        let emitter = self.selected_layer().id;
        self.execute(
            "Deleted module",
            EffectCommand::RemoveModule {
                emitter,
                module: id,
            },
            true,
        );
    }

    pub fn add_sprite_renderer(&mut self) {
        let emitter = self.selected_layer();
        let renderer = RendererInstance::sprite(BlendMode::Additive, 0.5);
        let renderer_id = renderer.id;
        if self.execute(
            "Added sprite renderer",
            EffectCommand::AddRenderer {
                emitter: emitter.id,
                renderer,
                index: emitter.renderers.len(),
            },
            true,
        ) {
            self.selection.primary = aestra_authoring::SemanticTarget::Renderer(renderer_id);
        }
    }

    pub fn toggle_renderer(&mut self, id: RendererId) {
        let emitter = self.selected_layer();
        let Some(renderer) = emitter.renderers.iter().find(|renderer| renderer.id == id) else {
            self.status = "Renderer no longer exists".into();
            return;
        };
        self.execute(
            "Toggled renderer",
            EffectCommand::SetRendererEnabled {
                emitter: emitter.id,
                renderer: id,
                enabled: !renderer.enabled,
            },
            true,
        );
    }

    pub fn cycle_renderer_blend(&mut self, id: RendererId) {
        let emitter = self.selected_layer();
        let Some(renderer) = emitter.renderers.iter().find(|renderer| renderer.id == id) else {
            self.status = "Renderer no longer exists".into();
            return;
        };
        let blend = match renderer.blend {
            BlendMode::Alpha => BlendMode::Additive,
            BlendMode::Additive => BlendMode::Multiply,
            BlendMode::Multiply => BlendMode::Alpha,
        };
        self.execute(
            "Changed renderer blend",
            EffectCommand::SetRendererBlend {
                emitter: emitter.id,
                renderer: id,
                blend,
            },
            true,
        );
    }

    pub fn adjust_renderer_softness(&mut self, id: RendererId, delta: f32) {
        let emitter = self.selected_layer();
        let Some(renderer) = emitter.renderers.iter().find(|renderer| renderer.id == id) else {
            self.status = "Renderer no longer exists".into();
            return;
        };
        let RendererProperties::Sprite { softness } = renderer.properties else {
            self.status = "This renderer has no softness property".into();
            return;
        };
        self.execute(
            "Changed renderer softness",
            EffectCommand::SetRendererProperties {
                emitter: emitter.id,
                renderer: id,
                properties: RendererProperties::Sprite {
                    softness: (softness + delta).max(0.0),
                },
            },
            true,
        );
    }

    pub fn duplicate_renderer(&mut self, id: RendererId) {
        let emitter = self.selected_layer().id;
        let Some(command) = EffectCommand::duplicate_renderer(&self.effect, emitter, id) else {
            self.status = "Renderer no longer exists".into();
            return;
        };
        self.execute("Duplicated renderer", command, true);
    }

    pub fn delete_renderer(&mut self, id: RendererId) {
        let emitter = self.selected_layer().id;
        self.execute(
            "Deleted renderer",
            EffectCommand::RemoveRenderer {
                emitter,
                renderer: id,
            },
            true,
        );
    }

    pub fn adjust_selected_start(&mut self, delta: f32) {
        let emitter = self.selected_layer();
        let start_time =
            (emitter.start_time + delta).clamp(0.0, (self.effect.duration - 0.05).max(0.0));
        let duration = emitter.duration.min(self.effect.duration - start_time);
        self.execute(
            "Moved layer",
            EffectCommand::SetEmitterTiming {
                id: emitter.id,
                start_time,
                duration,
            },
            true,
        );
    }

    pub fn adjust_selected_duration(&mut self, delta: f32) {
        let emitter = self.selected_layer();
        let duration =
            (emitter.duration + delta).clamp(0.05, self.effect.duration - emitter.start_time);
        self.execute(
            "Trimmed layer",
            EffectCommand::SetEmitterTiming {
                id: emitter.id,
                start_time: emitter.start_time,
                duration,
            },
            true,
        );
    }

    pub fn adjust_effect_duration(&mut self, delta: f32) {
        let duration = (self.effect.duration + delta).max(0.25);
        let mut commands = vec![EffectCommand::SetEffectDuration { duration }];
        commands.extend(self.effect.emitters.iter().map(|emitter| {
            let start_time = emitter.start_time.min((duration - 0.05).max(0.0));
            let emitter_duration = emitter.duration.min(duration - start_time).max(0.05);
            EffectCommand::SetEmitterTiming {
                id: emitter.id,
                start_time,
                duration: emitter_duration,
            }
        }));
        self.execute_transaction(
            EffectTransaction::new("Changed effect duration", commands),
            true,
        );
    }

    fn record_command_error(&mut self, prefix: &str, error: CommandError) {
        if let CommandError::Validation(report) = &error {
            self.diagnostics = report.clone();
        }
        self.status = format!("{prefix}: {error}");
        self.ui_revision += 1;
    }

    fn refresh_preview(&mut self) {
        match compile_preview(&self.effect) {
            Ok(preview) => {
                self.preview = Some(preview);
                self.diagnostics = self.effect.validation_report();
            }
            Err(error) => {
                self.preview = None;
                self.diagnostics = error.report().clone();
                self.samples.clear();
            }
        }
    }
}

fn compile_preview(effect: &EffectAsset) -> Result<EffectInstance, CompileError> {
    let compiled = EffectCompiler::default().compile(effect)?;
    Ok(EffectInstance::new(Arc::new(compiled)))
}

pub(crate) fn blank_effect() -> EffectAsset {
    let mut effect = EffectAsset::new("Untitled Effect", 2.0);
    effect.emitters.push(default_layer(0));
    effect
}

fn default_layer(index: usize) -> Emitter {
    Emitter::basic_sprite(format!("Emitter {}", index + 1), 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_effect_is_valid() {
        blank_effect().validate().unwrap();
    }

    #[test]
    fn edits_support_command_undo_and_redo() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let original = session.effect.emitters[0].spawn_rate();
        let module = session.effect.emitters[0]
            .module_by_type(aestra_bevy::MODULE_EMISSION)
            .unwrap()
            .id;
        session.set_module_parameter(module, "spawn_rate", Value::Scalar(77.0));
        assert_eq!(session.effect.emitters[0].spawn_rate(), 77.0);
        assert_eq!(preview_spawn_rate(&session), 77.0);
        session.undo();
        assert_eq!(session.effect.emitters[0].spawn_rate(), original);
        assert_eq!(preview_spawn_rate(&session), original);
        session.redo();
        assert_eq!(session.effect.emitters[0].spawn_rate(), 77.0);
        assert_eq!(preview_spawn_rate(&session), 77.0);
    }

    #[test]
    fn editor_save_is_loadable_by_shared_runtime() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        session.new_effect();
        session.add_layer();
        let path = std::env::temp_dir().join(format!(
            "aestra-editor-roundtrip-{}.aestra.ron",
            std::process::id()
        ));
        session.save_as(&path).unwrap();
        let loaded = EffectAsset::load_ron(&path).unwrap();
        assert_eq!(loaded.emitters.len(), 2);
        assert_eq!(loaded.name, "Untitled Effect");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn structural_layer_commands_are_reversible_and_keep_ids() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        session.new_effect();
        session.add_layer();
        let added = session.selected_layer().id;
        assert_eq!(session.effect.emitters.len(), 2);
        session.undo();
        assert_eq!(session.effect.emitters.len(), 1);
        session.redo();
        assert_eq!(session.effect.emitters.len(), 2);
        assert_eq!(session.effect.emitters[1].id, added);
        session.delete_selected_layer();
        assert_eq!(session.effect.emitters.len(), 1);
        session.undo();
        assert_eq!(session.effect.emitters.len(), 2);
        assert_eq!(session.effect.emitters[1].id, added);
    }

    #[test]
    fn module_stack_edits_recompile_and_are_reversible() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let original = session.selected_layer().modules[0].id;
        session.duplicate_module(original);
        assert_eq!(session.selected_layer().modules.len(), 6);
        assert!(session.preview.is_some());
        session.undo();
        assert_eq!(session.selected_layer().modules.len(), 5);

        session.delete_module(original);
        assert_eq!(session.selected_layer().modules.len(), 4);
        assert!(session.preview.is_none());
        assert!(!session.diagnostics.is_valid());
        session.undo();
        assert_eq!(session.selected_layer().modules.len(), 5);
        assert!(session.preview.is_some());
    }

    #[test]
    fn curve_key_edits_recompile_and_undo() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let module = session.effect.emitters[0]
            .module_by_type(aestra_bevy::MODULE_APPEARANCE)
            .unwrap()
            .id;
        let original = session.effect.emitters[0].size_curve().keys[1];
        session.set_curve_key(module, "size", 1, CurveKey::new(original.time, 18.0));
        assert_eq!(session.effect.emitters[0].size_curve().keys[1].value, 18.0);
        assert!(session.preview.is_some());
        session.undo();
        assert_eq!(session.effect.emitters[0].size_curve().keys[1], original);
        assert!(session.preview.is_some());
    }

    #[test]
    fn selection_uses_semantic_emitter_id() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let id = session.effect.emitters[2].id;
        session.select_layer(2);
        assert_eq!(session.selection.emitter(&session.effect), Some(id));
    }

    fn preview_spawn_rate(session: &EditorSession) -> f32 {
        let instruction = &session
            .preview
            .as_ref()
            .expect("valid editor effect has a compiled preview")
            .effect()
            .emitters[0]
            .execution
            .emitter_update[0];
        match instruction {
            aestra_runtime::Instruction::Emit { spawn_rate, .. } => *spawn_rate
                .constant_value()
                .expect("editor-authored spawn rate is constant"),
            _ => panic!("first emitter instruction must be emission"),
        }
    }
}
