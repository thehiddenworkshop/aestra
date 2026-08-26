use aestra_authoring::{
    CommandError, CommandHistory, EffectCommand, EffectDiff, EffectTransaction, LockState,
    Selection,
};
use aestra_bevy::{
    AssetError, EffectAsset, Emitter, Gradient, MODULE_APPEARANCE, MODULE_EMISSION,
    MODULE_INITIALIZE, ScalarRange, ValidationReport, Value,
};
use bevy::prelude::Resource;
use std::path::{Path, PathBuf};

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
    pub ui_revision: u64,
    history: CommandHistory,
}

impl EditorSession {
    pub fn from_embedded_sample(source: &str, path: impl Into<PathBuf>) -> Self {
        let effect = EffectAsset::from_ron(source)
            .expect("the bundled Prism Bloom sample must always be valid");
        let selection = Selection::for_effect(&effect);
        let diagnostics = effect.validation_report();
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
            ui_revision: 0,
            history: CommandHistory::default(),
        }
    }

    pub fn restart(&mut self) {
        self.time = 0.0;
        self.playing = true;
        self.status = "Choreography restarted".into();
    }

    pub fn new_effect(&mut self) {
        self.effect = blank_effect();
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

    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<(), AssetError> {
        let path = path.as_ref();
        self.effect = EffectAsset::load_ron(path)?;
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
                self.diagnostics = self.effect.validation_report();
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
                self.diagnostics = self.effect.validation_report();
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
                self.diagnostics = self.effect.validation_report();
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

    pub fn set_selected_module_parameter(
        &mut self,
        label: &str,
        module_type: &str,
        parameter: &str,
        value: Value,
    ) {
        let emitter = self.selected_layer();
        let Some(module) = emitter.module_by_type(module_type).map(|module| module.id) else {
            self.status = format!("Emitter is missing module '{module_type}'");
            return;
        };
        self.execute(
            label,
            EffectCommand::SetModuleParameter {
                emitter: emitter.id,
                module,
                parameter: parameter.into(),
                value,
            },
            true,
        );
    }

    pub fn adjust_spawn_rate(&mut self, delta: f32) {
        let value = (self.selected_layer().spawn_rate() + delta).max(0.0);
        self.set_selected_module_parameter(
            "Changed spawn rate",
            MODULE_EMISSION,
            "spawn_rate",
            Value::Scalar(value),
        );
    }

    pub fn adjust_burst_count(&mut self, delta: i32) {
        let current = self.selected_layer().burst_count();
        let value = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta as u32)
        };
        self.set_selected_module_parameter(
            "Changed burst count",
            MODULE_EMISSION,
            "burst_count",
            Value::U32(value),
        );
    }

    pub fn adjust_lifetime(&mut self, delta: f32) {
        let current = self.selected_layer().lifetime();
        let min = (current.min + delta).max(0.05);
        let max = (current.max + delta).max(min);
        self.set_selected_module_parameter(
            "Changed lifetime",
            MODULE_INITIALIZE,
            "lifetime",
            Value::Range(ScalarRange::new(min, max)),
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

    pub fn adjust_curve_key(
        &mut self,
        parameter: &str,
        key: usize,
        delta: f32,
        range: std::ops::RangeInclusive<f32>,
    ) {
        let mut curve = match parameter {
            "size" => self.selected_layer().size_curve().clone(),
            "opacity" => self.selected_layer().opacity_curve().clone(),
            _ => {
                self.status = format!("Unknown curve '{parameter}'");
                return;
            }
        };
        let Some(curve_key) = curve.keys.get_mut(key) else {
            self.status = format!("Curve key {key} does not exist");
            return;
        };
        curve_key.value = (curve_key.value + delta).clamp(*range.start(), *range.end());
        self.set_selected_module_parameter(
            "Edited curve",
            MODULE_APPEARANCE,
            parameter,
            Value::Curve(curve),
        );
    }

    pub fn set_color_gradient(&mut self, gradient: Gradient) {
        self.set_selected_module_parameter(
            "Changed color gradient",
            MODULE_APPEARANCE,
            "color",
            Value::Gradient(gradient),
        );
    }

    pub fn toggle_selected_layer(&mut self) {
        let emitter = self.selected_layer();
        self.execute(
            "Toggled layer visibility",
            EffectCommand::SetEmitterEnabled {
                id: emitter.id,
                enabled: !emitter.enabled,
            },
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
        session.set_selected_module_parameter(
            "Changed rate",
            MODULE_EMISSION,
            "spawn_rate",
            Value::Scalar(77.0),
        );
        assert_eq!(session.effect.emitters[0].spawn_rate(), 77.0);
        session.undo();
        assert_eq!(session.effect.emitters[0].spawn_rate(), original);
        session.redo();
        assert_eq!(session.effect.emitters[0].spawn_rate(), 77.0);
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
    fn selection_uses_semantic_emitter_id() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let id = session.effect.emitters[2].id;
        session.select_layer(2);
        assert_eq!(session.selection.emitter(&session.effect), Some(id));
    }
}
