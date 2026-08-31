use aestra_authoring::{
    CommandError, CommandExecutor, CommandHistory, EffectCommand, EffectDiff, EffectTransaction,
    LockState, Selection, TransactionPreview,
};
use aestra_bevy::{
    AssetError, AssetId, AssetKind, BlendMode, ColorKey, CurveKey, EffectAsset, EffectClipId,
    EffectParameter, Emitter, EmitterId, EmitterTransform, EventId, EventLink, EventTrigger,
    FlipbookDefinition, FlipbookPlaybackMode, FlipbookTimeSource, MaterialDefinition, MaterialId,
    MaterialInput, MaterialProperties, ModuleId, ModuleInstance, RendererId, RendererInstance,
    RendererProperties, ValidationReport, Value,
};
use aestra_compiler::{CompileError, EffectCompiler};
use aestra_runtime::{
    CheckpointBackendId, CheckpointContext, CheckpointStore, EffectInstance, PlaybackClock,
    SeekOrigin, SeekPlan, SimulationSeekMode,
};
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

pub(crate) struct PendingChange {
    pub preview: TransactionPreview,
    pub diagnostics: ValidationReport,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventLinkError {
    SameEmitter,
    Duplicate,
    TargetMissing,
}

#[derive(Resource)]
pub(crate) struct EditorSession {
    pub effect: EffectAsset,
    pub source_path: Option<PathBuf>,
    pub selection: Selection,
    pub locks: LockState,
    pub diagnostics: ValidationReport,
    pub last_diff: EffectDiff,
    pub pending_change: Option<PendingChange>,
    pub clock: PlaybackClock,
    pub preview_seed: u64,
    pub solo_emitter: Option<EmitterId>,
    pub playing: bool,
    pub speed: f32,
    pub dirty: bool,
    pub status: String,
    pub samples: Vec<aestra_bevy::ParticleSample>,
    pub preview: Option<EffectInstance>,
    pub ui_revision: u64,
    history: CommandHistory,
    saved_effect: Option<EffectAsset>,
    checkpoints: CheckpointStore<EffectInstance>,
    effect_revision: u64,
    last_seek: SeekPlan,
}

impl EditorSession {
    pub fn from_embedded_sample(source: &str, path: impl Into<PathBuf>) -> Self {
        let effect = EffectAsset::from_ron(source)
            .expect("the bundled Prism Bloom sample must always be valid");
        let selection = Selection::for_effect(&effect);
        let diagnostics = effect.validation_report();
        let preview_seed = 0;
        let preview = compile_preview(&effect, preview_seed)
            .expect("the bundled Prism Bloom sample must always compile");
        let saved_effect = effect.clone();
        Self {
            effect,
            source_path: Some(path.into()),
            selection,
            locks: LockState::default(),
            diagnostics,
            last_diff: EffectDiff::default(),
            pending_change: None,
            clock: PlaybackClock::default(),
            preview_seed,
            solo_emitter: None,
            playing: true,
            speed: 1.0,
            dirty: false,
            status: "Previewing embedded Prism Bloom".into(),
            samples: Vec::with_capacity(384),
            preview: Some(preview),
            ui_revision: 0,
            history: CommandHistory::default(),
            saved_effect: Some(saved_effect),
            checkpoints: CheckpointStore::default(),
            effect_revision: 0,
            last_seek: direct_seek_plan(0),
        }
    }

    pub fn restart(&mut self) {
        self.clock.restart();
        if let Some(preview) = &mut self.preview {
            preview.restart();
            preview.set_seed(self.preview_seed);
        }
        self.last_seek = SeekPlan {
            target_frame: 0,
            origin: SeekOrigin::Restart,
            replay_ticks: 0,
        };
        self.playing = true;
        self.status = "Choreography restarted".into();
    }

    pub fn stop(&mut self) {
        self.clock.restart();
        if let Some(preview) = &mut self.preview {
            preview.restart();
            preview.set_seed(self.preview_seed);
        }
        self.last_seek = SeekPlan {
            target_frame: 0,
            origin: SeekOrigin::Restart,
            replay_ticks: 0,
        };
        self.playing = false;
        self.status = "Choreography stopped".into();
    }

    pub fn time(&self) -> f32 {
        self.clock.time(self.playback_duration())
    }

    pub fn frame(&self) -> u64 {
        self.clock.frame()
    }

    pub fn playback_duration(&self) -> f32 {
        self.pending_change
            .as_ref()
            .filter(|pending| pending.can_apply)
            .map_or(self.effect.duration, |pending| {
                pending.preview.candidate().duration
            })
    }

    fn playback_looping(&self) -> bool {
        self.pending_change
            .as_ref()
            .filter(|pending| pending.can_apply)
            .map_or(self.effect.looping, |pending| {
                pending.preview.candidate().looping
            })
    }

    pub fn seek_mode(&self) -> SimulationSeekMode {
        self.preview
            .as_ref()
            .map_or(SimulationSeekMode::RestartReplay, |preview| {
                preview.effect().seek_mode
            })
    }

    pub fn seek_status(&self) -> String {
        match self.seek_mode() {
            SimulationSeekMode::StatelessDirect => "DIRECT SEEK · STATELESS".into(),
            SimulationSeekMode::CheckpointRestore => format!(
                "{} CHECKPOINTS · {} KB · {}",
                self.checkpoints.len(),
                self.checkpoints.estimated_bytes().div_ceil(1024),
                seek_origin_label(self.last_seek.origin)
            ),
            SimulationSeekMode::RestartReplay => format!(
                "RESTART + REPLAY FALLBACK · {}",
                seek_origin_label(self.last_seek.origin)
            ),
        }
    }

    pub fn seek_time(&mut self, time: f32) {
        let duration = self.playback_duration();
        let mut target = self.clock;
        target.seek_seconds(time, duration);
        self.seek_frame(target.frame());
    }

    pub fn step_frame(&mut self, direction: i8) {
        let target = if direction < 0 {
            self.frame().saturating_sub(1)
        } else {
            self.frame().saturating_add(1)
        };
        self.seek_frame(target);
    }

    pub fn toggle_preview_solo(&mut self, emitter: EmitterId) -> bool {
        if !self.effect.emitters.iter().any(|item| item.id == emitter) {
            return false;
        }
        self.solo_emitter = (self.solo_emitter != Some(emitter)).then_some(emitter);
        self.checkpoints.clear();
        self.clock.restart();
        self.refresh_preview();
        self.status = if self.solo_emitter.is_some() {
            "Soloing emitter in preview".into()
        } else {
            "Emitter solo cleared".into()
        };
        self.ui_revision += 1;
        true
    }

    pub fn advance_playback(&mut self, delta_seconds: f32) {
        if !self.playing {
            return;
        }
        let duration = self.playback_duration();
        let looping = self.playback_looping();
        let result = self
            .clock
            .advance(delta_seconds, self.speed, duration, looping);
        if self.seek_mode() != SimulationSeekMode::StatelessDirect
            && let Some(preview) = &mut self.preview
        {
            let tick_seconds = 1.0 / self.clock.tick_rate() as f32;
            for _ in 0..result.ticks {
                preview.advance(tick_seconds);
            }
        }
        if result.reached_end {
            self.playing = false;
        }
    }

    pub fn evaluate_preview(&mut self, output: &mut Vec<aestra_bevy::ParticleSample>) {
        let time = self.time();
        let mode = self.seek_mode();
        let Some(preview) = &mut self.preview else {
            output.clear();
            return;
        };
        if mode == SimulationSeekMode::StatelessDirect {
            preview.seek(time);
        }
        preview.evaluate(output);
        self.record_checkpoint_if_due();
    }

    fn seek_frame(&mut self, target_frame: u64) {
        let duration = self.playback_duration();
        let target_frame = target_frame.min(self.clock.maximum_frame(duration));
        let context = self.checkpoint_context();
        let mode = self.seek_mode();
        let plan = self
            .checkpoints
            .plan_seek(mode, &context, self.frame(), target_frame);
        let restored = match plan.origin {
            SeekOrigin::Checkpoint { frame } => self
                .checkpoints
                .nearest_at_or_before(&context, frame)
                .map(|checkpoint| checkpoint.state.clone()),
            _ => None,
        };

        match plan.origin {
            SeekOrigin::Direct => {
                self.clock.seek_frame(target_frame, duration);
                let time = self.time();
                if let Some(preview) = &mut self.preview {
                    preview.seek(time);
                }
            }
            SeekOrigin::Current => self.replay_ticks(plan.replay_ticks),
            SeekOrigin::Checkpoint { frame } => {
                if let Some(preview) = restored {
                    self.preview = Some(preview);
                    self.clock.seek_frame(frame, duration);
                    self.replay_ticks(plan.replay_ticks);
                } else {
                    self.restart_for_replay();
                    self.replay_ticks(target_frame);
                }
            }
            SeekOrigin::Restart => {
                self.restart_for_replay();
                self.replay_ticks(plan.replay_ticks);
            }
        }
        self.last_seek = plan;
        self.playing = false;
        self.status = format!(
            "Scrubbed to frame {} ({:.3}s) via {}",
            self.frame(),
            self.time(),
            seek_origin_label(plan.origin)
        );
    }

    fn replay_ticks(&mut self, ticks: u64) {
        let duration = self.playback_duration();
        let tick_seconds = 1.0 / self.clock.tick_rate() as f32;
        for _ in 0..ticks {
            self.clock.step_forward(duration);
            if let Some(preview) = &mut self.preview {
                preview.advance(tick_seconds);
            }
            self.record_checkpoint_if_due();
        }
    }

    fn restart_for_replay(&mut self) {
        self.clock.restart();
        if let Some(preview) = &mut self.preview {
            preview.restart();
            preview.set_seed(self.preview_seed);
        }
    }

    fn restore_preview_frame(&mut self, frame: u64) {
        let duration = self.playback_duration();
        let frame = frame.min(self.clock.maximum_frame(duration));
        match self.seek_mode() {
            SimulationSeekMode::StatelessDirect => {
                self.clock.seek_frame(frame, duration);
                let time = self.time();
                if let Some(preview) = &mut self.preview {
                    preview.seek(time);
                }
                self.last_seek = direct_seek_plan(frame);
            }
            SimulationSeekMode::CheckpointRestore | SimulationSeekMode::RestartReplay => {
                self.restart_for_replay();
                self.replay_ticks(frame);
                self.last_seek = SeekPlan {
                    target_frame: frame,
                    origin: SeekOrigin::Restart,
                    replay_ticks: frame,
                };
            }
        }
    }

    fn record_checkpoint_if_due(&mut self) {
        if self.seek_mode() != SimulationSeekMode::CheckpointRestore
            || !self.checkpoints.policy().should_capture(self.frame())
        {
            return;
        }
        let Some(preview) = self.preview.clone() else {
            return;
        };
        let estimated_bytes = std::mem::size_of::<EffectInstance>()
            + std::mem::size_of_val(preview.parameter_values());
        self.checkpoints.insert(
            self.checkpoint_context(),
            self.frame(),
            preview,
            estimated_bytes,
        );
    }

    fn checkpoint_context(&self) -> CheckpointContext {
        let effect = self
            .pending_change
            .as_ref()
            .filter(|pending| pending.can_apply)
            .map_or(self.effect.id, |pending| pending.preview.candidate().id);
        CheckpointContext {
            effect,
            revision: self.effect_revision
                + u64::from(
                    self.pending_change
                        .as_ref()
                        .is_some_and(|pending| pending.can_apply),
                ),
            seed: self.preview_seed,
            backend: CheckpointBackendId::new("cpu-reference"),
        }
    }

    pub fn new_effect(&mut self) {
        self.effect = blank_effect();
        self.solo_emitter = None;
        self.invalidate_effect_checkpoints();
        self.preview = Some(
            compile_preview(&self.effect, self.preview_seed).expect("blank effect must compile"),
        );
        self.source_path = None;
        self.selection = Selection::for_effect(&self.effect);
        self.locks = LockState::default();
        self.diagnostics = self.effect.validation_report();
        self.last_diff = EffectDiff::default();
        self.pending_change = None;
        self.clock.restart();
        self.playing = false;
        self.dirty = true;
        self.saved_effect = None;
        self.history.clear();
        self.ui_revision += 1;
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let path = path.as_ref();
        let effect = EffectAsset::load_ron(path)?;
        let preview = compile_preview(&effect, self.preview_seed)?;
        self.saved_effect = Some(effect.clone());
        self.effect = effect;
        self.solo_emitter = None;
        self.invalidate_effect_checkpoints();
        self.preview = Some(preview);
        self.source_path = Some(path.to_owned());
        self.selection = Selection::for_effect(&self.effect);
        self.locks = LockState::default();
        self.diagnostics = self.effect.validation_report();
        self.last_diff = EffectDiff::default();
        self.pending_change = None;
        self.clock.restart();
        self.playing = false;
        self.dirty = false;
        self.history.clear();
        self.ui_revision += 1;
        Ok(())
    }

    pub fn restore_recovery(&mut self, effect: EffectAsset, source_path: Option<PathBuf>) {
        let preview = compile_preview(&effect, self.preview_seed).ok();
        let saved_effect = source_path
            .as_deref()
            .and_then(|path| EffectAsset::load_ron(path).ok());
        self.effect = effect;
        self.solo_emitter = None;
        self.invalidate_effect_checkpoints();
        self.preview = preview;
        self.source_path = source_path;
        self.selection = Selection::for_effect(&self.effect);
        self.locks = LockState::default();
        self.diagnostics = self.effect.validation_report();
        self.last_diff = EffectDiff::default();
        self.pending_change = None;
        self.clock.restart();
        self.playing = false;
        self.saved_effect = saved_effect;
        self.update_dirty_state();
        self.history.clear();
        self.ui_revision += 1;
    }

    pub fn document_revision(&self) -> u64 {
        self.effect_revision
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
        self.saved_effect = Some(self.effect.clone());
        self.update_dirty_state();
        self.ui_revision += 1;
        Ok(())
    }

    /// Accepts a successful external rename of the clean source currently open in the editor.
    ///
    /// Library asset operations save the renamed source atomically before updating the session,
    /// so this only realigns the in-memory document identity and clean baseline.
    pub(crate) fn accept_external_source_rename(
        &mut self,
        path: impl Into<PathBuf>,
        name: impl Into<String>,
    ) {
        self.effect.name = name.into();
        self.source_path = Some(path.into());
        self.saved_effect = Some(self.effect.clone());
        self.update_dirty_state();
        self.ui_revision += 1;
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
        let discarded_preview = self.pending_change.take().is_some();
        let label = transaction.label.clone();
        match self
            .history
            .execute(&mut self.effect, &self.locks, transaction)
        {
            Ok(diff) => {
                self.last_diff = diff;
                self.invalidate_effect_checkpoints();
                self.refresh_preview();
                self.selection.repair(&self.effect);
                self.clamp_clock();
                self.update_dirty_state();
                self.status = label;
                if rebuild_ui {
                    self.ui_revision += 1;
                }
                true
            }
            Err(error) => {
                if discarded_preview {
                    self.refresh_preview();
                }
                self.record_command_error("Edit failed", error);
                false
            }
        }
    }

    pub fn set_selected_emitter_transform(
        &mut self,
        transform: EmitterTransform,
        rebuild_ui: bool,
    ) -> bool {
        self.execute(
            "Transformed emitter",
            EffectCommand::SetEmitterTransform {
                id: self.selected_layer().id,
                transform,
            },
            rebuild_ui,
        )
    }

    /// Compiles a temporary interaction candidate without mutating the document or history.
    /// Viewport gizmos use this while dragging, then commit one normal command on release.
    pub fn preview_interaction(&mut self, transaction: EffectTransaction) -> bool {
        let preview = match CommandExecutor::preview(&self.effect, &self.locks, transaction) {
            Ok(preview) => preview,
            Err(_) => return false,
        };
        let Ok(runtime_preview) =
            compile_preview_with_solo(preview.candidate(), self.preview_seed, self.solo_emitter)
        else {
            return false;
        };
        self.preview = Some(runtime_preview);
        self.samples.clear();
        self.checkpoints.clear();
        true
    }

    pub fn restore_interaction_preview(&mut self) {
        self.refresh_preview();
    }

    pub fn preview_transaction(&mut self, transaction: EffectTransaction) -> bool {
        let label = transaction.label.clone();
        let preview = match CommandExecutor::preview(&self.effect, &self.locks, transaction) {
            Ok(preview) => preview,
            Err(error) => {
                self.record_command_error("Preview failed", error);
                return false;
            }
        };
        let (diagnostics, can_apply) = match compile_preview_with_solo(
            preview.candidate(),
            self.preview_seed,
            self.solo_emitter,
        ) {
            Ok(runtime_preview) => {
                self.preview = Some(runtime_preview);
                self.samples.clear();
                self.clock.restart();
                (preview.candidate().validation_report(), true)
            }
            Err(error) => {
                self.preview =
                    compile_preview_with_solo(&self.effect, self.preview_seed, self.solo_emitter)
                        .ok();
                (error.report().clone(), false)
            }
        };
        self.checkpoints.clear();
        let change_count = preview.diff().changes.len();
        self.pending_change = Some(PendingChange {
            preview,
            diagnostics,
            can_apply,
        });
        self.status = if can_apply {
            format!("Reviewing {label} ({change_count} changes)")
        } else {
            format!("{label} has compiler errors and cannot be applied")
        };
        self.ui_revision += 1;
        true
    }

    pub fn apply_pending_change(&mut self) -> bool {
        let Some(pending) = self.pending_change.take() else {
            self.status = "There is no transaction to apply".into();
            return false;
        };
        if !pending.can_apply {
            self.status = "Resolve the preview diagnostics before applying".into();
            self.pending_change = Some(pending);
            self.ui_revision += 1;
            return false;
        }
        let label = pending.preview.transaction().label.clone();
        match self
            .history
            .commit_preview(&mut self.effect, &self.locks, pending.preview)
        {
            Ok(diff) => {
                self.last_diff = diff;
                self.invalidate_effect_checkpoints();
                self.selection.repair(&self.effect);
                self.refresh_preview();
                self.clamp_clock();
                self.update_dirty_state();
                self.status = format!("Applied {label}");
                self.ui_revision += 1;
                true
            }
            Err(error) => {
                self.refresh_preview();
                self.record_command_error("Apply failed", error);
                false
            }
        }
    }

    pub fn discard_pending_change(&mut self) -> bool {
        let Some(pending) = self.pending_change.take() else {
            self.status = "There is no transaction to discard".into();
            return false;
        };
        let label = pending.preview.transaction().label.clone();
        self.checkpoints.clear();
        self.refresh_preview();
        self.clamp_clock();
        self.status = format!("Discarded {label}");
        self.ui_revision += 1;
        true
    }

    pub fn undo(&mut self) {
        if self.pending_change.take().is_some() {
            self.checkpoints.clear();
            self.refresh_preview();
        }
        match self.history.undo(&mut self.effect) {
            Ok(Some(result)) => {
                self.selection.repair(&self.effect);
                self.invalidate_effect_checkpoints();
                self.refresh_preview();
                self.last_diff = result.diff;
                self.clamp_clock();
                self.status = format!("Undid {}", result.label);
                self.update_dirty_state();
                self.ui_revision += 1;
            }
            Ok(None) => self.status = "Nothing to undo".into(),
            Err(error) => self.record_command_error("Undo failed", error),
        }
    }

    pub fn redo(&mut self) {
        if self.pending_change.take().is_some() {
            self.checkpoints.clear();
            self.refresh_preview();
        }
        match self.history.redo(&mut self.effect) {
            Ok(Some(result)) => {
                self.selection.repair(&self.effect);
                self.invalidate_effect_checkpoints();
                self.refresh_preview();
                self.last_diff = result.diff;
                self.clamp_clock();
                self.status = format!("Redid {}", result.label);
                self.update_dirty_state();
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
            .or_else(|| self.effect.emitters.first().map(|emitter| emitter.id))
            .expect("the editor always contains at least one emitter");
        self.effect
            .emitters
            .iter()
            .position(|emitter| emitter.id == id)
            .expect("selected emitter must exist")
    }

    pub fn selected_layer(&self) -> &Emitter {
        &self.effect.emitters[self.selected_layer_index()]
    }

    pub fn select_emitter(&mut self, id: EmitterId) -> bool {
        let Some(emitter) = self.effect.emitters.iter().find(|emitter| emitter.id == id) else {
            self.status = "Emitter no longer exists".into();
            return false;
        };
        if self.selection.primary == aestra_authoring::SemanticTarget::Emitter(id) {
            return false;
        }
        let name = emitter.name.clone();
        self.selection.select_emitter(id);
        self.status = format!("Selected {name}");
        self.ui_revision += 1;
        true
    }

    pub fn select_effect_clip(&mut self, id: EffectClipId) -> bool {
        let Some(clip) = self.effect.effect_clips.iter().find(|clip| clip.id == id) else {
            self.status = "Effect clip no longer exists".into();
            return false;
        };
        if self.selection.primary == aestra_authoring::SemanticTarget::EffectClip(id) {
            return false;
        }
        let source = clip.source;
        self.selection.select_effect_clip(id);
        self.status = format!("Selected effect clip {source}");
        self.ui_revision += 1;
        true
    }

    pub fn select_marker(&mut self, id: aestra_bevy::MarkerId) -> bool {
        let Some(marker) = self.effect.markers.iter().find(|marker| marker.id == id) else {
            self.status = "Marker no longer exists".into();
            return false;
        };
        if self.selection.primary == aestra_authoring::SemanticTarget::Marker(id) {
            return false;
        }
        let name = marker.name.clone();
        self.selection.select_marker(id);
        self.status = format!("Selected marker {name}");
        self.ui_revision += 1;
        true
    }

    pub fn select_choreography_event(&mut self, id: aestra_bevy::ChoreographyEventId) -> bool {
        let Some(event) = self
            .effect
            .choreography_events
            .iter()
            .find(|event| event.id == id)
        else {
            self.status = "Choreography event no longer exists".into();
            return false;
        };
        if self.selection.primary == aestra_authoring::SemanticTarget::ChoreographyEvent(id) {
            return false;
        }
        let name = event.name.clone();
        self.selection.select_choreography_event(id);
        self.status = format!("Selected choreography event {name}");
        self.ui_revision += 1;
        true
    }

    pub fn add_layer(&mut self) {
        let index = self.effect.emitters.len();
        let mut emitter = default_layer(index);
        emitter.duration = self.effect.duration;
        if let (Some(renderer), Some(material)) =
            (emitter.renderers.first_mut(), self.effect.materials.first())
        {
            renderer.material = material.id;
        }
        let id = emitter.id;
        if self.execute(
            "Added emitter layer",
            EffectCommand::AddEmitter { emitter, index },
            true,
        ) {
            self.selection.select_emitter(id);
        }
    }

    pub fn set_effect_name(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.effect.name == name {
            return false;
        }
        self.execute(
            "Renamed effect",
            EffectCommand::SetEffectName { name },
            true,
        )
    }

    pub fn set_effect_looping(&mut self, looping: bool) -> bool {
        if self.effect.looping == looping {
            return false;
        }
        let frame = self.frame();
        let playing = self.playing;
        let changed = self.execute(
            "Changed effect looping",
            EffectCommand::SetEffectLooping { looping },
            true,
        );
        if changed {
            self.restore_preview_frame(frame);
            self.playing = playing;
            self.status = "Changed effect looping".into();
        }
        changed
    }

    pub fn set_selected_emitter_name(&mut self, name: impl Into<String>) -> bool {
        self.set_emitter_name(self.selected_layer().id, name)
    }

    pub fn set_emitter_name(&mut self, id: EmitterId, name: impl Into<String>) -> bool {
        let name = name.into();
        if self
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == id)
            .is_none_or(|emitter| emitter.name == name)
        {
            return false;
        }
        self.execute(
            "Renamed emitter",
            EffectCommand::SetEmitterName { id, name },
            true,
        )
    }

    pub fn set_selected_emitter_enabled(&mut self, enabled: bool) -> bool {
        let emitter = self.selected_layer();
        if emitter.enabled == enabled {
            return false;
        }
        self.execute(
            "Changed emitter enabled state",
            EffectCommand::SetEmitterEnabled {
                id: emitter.id,
                enabled,
            },
            true,
        )
    }

    pub fn set_emitter_display_color(&mut self, id: EmitterId, color: Option<[f32; 4]>) -> bool {
        if self
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == id)
            .is_none_or(|emitter| emitter.display_color == color)
        {
            return false;
        }
        self.execute(
            "Changed emitter display color",
            EffectCommand::SetEmitterDisplayColor { id, color },
            true,
        )
    }

    pub fn set_selected_emitter_capacity(&mut self, max_particles: u32) -> bool {
        let emitter = self.selected_layer();
        if emitter.max_particles == max_particles {
            return false;
        }
        self.execute(
            "Changed emitter capacity",
            EffectCommand::SetEmitterCapacity {
                id: emitter.id,
                max_particles,
            },
            true,
        )
    }

    pub fn add_event_link(
        &mut self,
        trigger: EventTrigger,
        target: EmitterId,
    ) -> Result<EventId, EventLinkError> {
        let source = self.selected_layer().id;
        if source == target {
            return Err(EventLinkError::SameEmitter);
        }
        if !self
            .effect
            .emitters
            .iter()
            .any(|emitter| emitter.id == target)
        {
            return Err(EventLinkError::TargetMissing);
        }
        if self.effect.events.iter().any(|event| {
            event.source == source && event.target == target && event.trigger == trigger
        }) {
            return Err(EventLinkError::Duplicate);
        }
        let event = EventLink {
            id: EventId::new(),
            source,
            trigger,
            target,
        };
        let id = event.id;
        if self.execute(
            "Added event link",
            EffectCommand::AddEvent {
                event,
                index: self.effect.events.len(),
            },
            true,
        ) {
            Ok(id)
        } else {
            Err(EventLinkError::TargetMissing)
        }
    }

    pub fn remove_event_link(&mut self, id: EventId) -> bool {
        if !self.effect.events.iter().any(|event| event.id == id) {
            return false;
        }
        self.execute(
            "Removed event link",
            EffectCommand::RemoveEvent { id },
            true,
        )
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
        if let Some(mut effect_parameter) = self.bound_effect_parameter(module, parameter) {
            let Value::Curve(curve) = &mut effect_parameter.default else {
                return;
            };
            if index > curve.keys.len() {
                return;
            }
            curve.keys.insert(index, key);
            let id = effect_parameter.id;
            self.execute(
                format!("Added {parameter} curve key"),
                EffectCommand::SetParameter {
                    id,
                    parameter: effect_parameter,
                },
                true,
            );
            return;
        }
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
        if let Some(mut effect_parameter) = self.bound_effect_parameter(module, parameter) {
            let Value::Curve(curve) = &mut effect_parameter.default else {
                return;
            };
            let Some(previous) = curve.keys.get_mut(index) else {
                return;
            };
            *previous = key;
            let id = effect_parameter.id;
            self.execute(
                format!("Changed {parameter} curve key"),
                EffectCommand::SetParameter {
                    id,
                    parameter: effect_parameter,
                },
                true,
            );
            return;
        }
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
        if let Some(mut effect_parameter) = self.bound_effect_parameter(module, parameter) {
            let Value::Curve(curve) = &mut effect_parameter.default else {
                return;
            };
            if index >= curve.keys.len() {
                return;
            }
            curve.keys.remove(index);
            let id = effect_parameter.id;
            self.execute(
                format!("Removed {parameter} curve key"),
                EffectCommand::SetParameter {
                    id,
                    parameter: effect_parameter,
                },
                true,
            );
            return;
        }
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
        if let Some(mut effect_parameter) = self.bound_effect_parameter(module, parameter) {
            let Value::Gradient(gradient) = &mut effect_parameter.default else {
                return;
            };
            if index > gradient.keys.len() {
                return;
            }
            gradient.keys.insert(index, key);
            let id = effect_parameter.id;
            self.execute(
                format!("Added {parameter} gradient key"),
                EffectCommand::SetParameter {
                    id,
                    parameter: effect_parameter,
                },
                true,
            );
            return;
        }
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
        if let Some(mut effect_parameter) = self.bound_effect_parameter(module, parameter) {
            let Value::Gradient(gradient) = &mut effect_parameter.default else {
                return;
            };
            let Some(previous) = gradient.keys.get_mut(index) else {
                return;
            };
            *previous = key;
            let id = effect_parameter.id;
            self.execute(
                format!("Changed {parameter} gradient key"),
                EffectCommand::SetParameter {
                    id,
                    parameter: effect_parameter,
                },
                true,
            );
            return;
        }
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
        if let Some(mut effect_parameter) = self.bound_effect_parameter(module, parameter) {
            let Value::Gradient(gradient) = &mut effect_parameter.default else {
                return;
            };
            if index >= gradient.keys.len() {
                return;
            }
            gradient.keys.remove(index);
            let id = effect_parameter.id;
            self.execute(
                format!("Removed {parameter} gradient key"),
                EffectCommand::SetParameter {
                    id,
                    parameter: effect_parameter,
                },
                true,
            );
            return;
        }
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

    fn bound_effect_parameter(&self, module: ModuleId, input: &str) -> Option<EffectParameter> {
        let parameter_id = self
            .selected_layer()
            .modules
            .iter()
            .find(|candidate| candidate.id == module)?
            .bindings
            .get(input)?;
        self.effect
            .parameters
            .iter()
            .find(|parameter| parameter.id == *parameter_id)
            .cloned()
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

    pub fn add_sprite_renderer(&mut self) {
        let emitter = self.selected_layer();
        let Some(material) = self.effect.materials.first().map(|material| material.id) else {
            self.status = "This effect has no sprite material".into();
            return;
        };
        let renderer = RendererInstance::sprite(material);
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

    pub fn add_grid_flipbook(&mut self) {
        let Some(texture) = self
            .effect
            .assets
            .iter()
            .find(|asset| asset.kind == AssetKind::Texture)
            .map(|asset| asset.id)
        else {
            self.status = "Import a texture before creating a flipbook".into();
            return;
        };
        let flipbook = FlipbookDefinition::grid(
            format!("Flipbook {}", self.effect.flipbooks.len() + 1),
            texture,
            4,
            1,
            12.0,
        );
        let id = flipbook.id;
        if self.execute(
            "Added grid flipbook",
            EffectCommand::AddFlipbook {
                flipbook,
                index: self.effect.flipbooks.len(),
            },
            true,
        ) {
            self.status = format!("Added flipbook {id}");
        }
    }

    pub fn add_flipbook_renderer(&mut self) {
        let emitter = self.selected_layer();
        let Some(material) = self.effect.materials.first().map(|material| material.id) else {
            self.status = "This effect has no sprite material".into();
            return;
        };
        let Some(flipbook) = self.effect.flipbooks.first().map(|flipbook| flipbook.id) else {
            self.status = "Create a flipbook asset first".into();
            return;
        };
        let renderer = RendererInstance::flipbook(material, flipbook);
        let renderer_id = renderer.id;
        if self.execute(
            "Added flipbook renderer",
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

    pub fn set_renderer_flipbook(&mut self, id: RendererId, flipbook: AssetId) {
        self.update_flipbook_renderer(id, "Changed renderer flipbook", |properties| {
            if let RendererProperties::Flipbook {
                flipbook: current, ..
            } = properties
            {
                *current = flipbook;
            }
        });
    }

    pub fn set_flipbook_frame_rate(&mut self, id: RendererId, value: f32) {
        let Some(renderer) = self
            .selected_layer()
            .renderers
            .iter()
            .find(|item| item.id == id)
        else {
            return;
        };
        let RendererProperties::Flipbook { flipbook, .. } = renderer.properties else {
            return;
        };
        let Some(mut definition) = self
            .effect
            .flipbooks
            .iter()
            .find(|item| item.id == flipbook)
            .cloned()
        else {
            return;
        };
        let value = value.clamp(1.0, 120.0);
        if definition.frame_rate == value {
            return;
        }
        definition.frame_rate = value;
        self.execute(
            "Changed flipbook frame rate",
            EffectCommand::SetFlipbook {
                id: flipbook,
                flipbook: definition,
            },
            true,
        );
    }

    pub fn toggle_flipbook_looping(&mut self, id: RendererId) {
        let Some(renderer) = self
            .selected_layer()
            .renderers
            .iter()
            .find(|item| item.id == id)
        else {
            return;
        };
        let RendererProperties::Flipbook { flipbook, .. } = renderer.properties else {
            return;
        };
        let Some(mut definition) = self
            .effect
            .flipbooks
            .iter()
            .find(|item| item.id == flipbook)
            .cloned()
        else {
            return;
        };
        definition.looping = !definition.looping;
        self.execute(
            "Toggled flipbook looping",
            EffectCommand::SetFlipbook {
                id: flipbook,
                flipbook: definition,
            },
            true,
        );
    }

    pub fn set_flipbook_time_source(&mut self, id: RendererId, value: FlipbookTimeSource) {
        self.update_flipbook_renderer(id, "Changed flipbook time source", |properties| {
            if let RendererProperties::Flipbook { time_source, .. } = properties {
                *time_source = value;
            }
        });
    }

    pub fn set_flipbook_playback(&mut self, id: RendererId, value: FlipbookPlaybackMode) {
        self.update_flipbook_renderer(id, "Changed flipbook playback", |properties| {
            if let RendererProperties::Flipbook { playback, .. } = properties {
                *playback = value;
            }
        });
    }

    pub fn toggle_flipbook_random_start(&mut self, id: RendererId) {
        self.update_flipbook_renderer(id, "Toggled random flipbook start", |properties| {
            if let RendererProperties::Flipbook { random_start, .. } = properties {
                *random_start = !*random_start;
            }
        });
    }

    fn update_flipbook_renderer(
        &mut self,
        id: RendererId,
        label: &str,
        update: impl FnOnce(&mut RendererProperties),
    ) {
        let emitter = self.selected_layer();
        let emitter_id = emitter.id;
        let Some(mut properties) = emitter
            .renderers
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.properties.clone())
        else {
            return;
        };
        update(&mut properties);
        self.execute(
            label,
            EffectCommand::SetRendererProperties {
                emitter: emitter_id,
                renderer: id,
                properties,
            },
            true,
        );
    }

    pub fn add_sprite_material(&mut self) {
        let material = MaterialDefinition::sprite(
            format!("Sprite Material {}", self.effect.materials.len() + 1),
            BlendMode::Additive,
            0.5,
        );
        let material_id = material.id;
        if self.execute(
            "Added sprite material",
            EffectCommand::AddMaterial {
                material,
                index: self.effect.materials.len(),
            },
            true,
        ) {
            self.status = format!("Added sprite material {material_id}");
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

    pub fn set_renderer_material(&mut self, id: RendererId, material: MaterialId) {
        let emitter = self.selected_layer().id;
        let Some(renderer) = self
            .selected_layer()
            .renderers
            .iter()
            .find(|renderer| renderer.id == id)
        else {
            self.status = "Renderer no longer exists".into();
            return;
        };
        if renderer.material == material {
            return;
        }
        self.execute(
            "Changed renderer material",
            EffectCommand::SetRendererMaterial {
                emitter,
                renderer: id,
                material,
            },
            true,
        );
    }

    pub fn set_renderer_blend(&mut self, id: RendererId, blend: BlendMode) {
        let Some(mut material) = self.renderer_material(id).cloned() else {
            self.status = "Renderer material no longer exists".into();
            return;
        };
        if material.blend == blend {
            return;
        }
        material.blend = blend;
        self.update_material("Changed material blend", material);
    }

    pub fn set_renderer_softness(&mut self, id: RendererId, value: f32) {
        let Some(mut material) = self.renderer_material(id).cloned() else {
            self.status = "Renderer material no longer exists".into();
            return;
        };
        let MaterialProperties::Sprite { softness, .. } = &mut material.properties;
        let MaterialInput::Constant(current) = softness else {
            self.status = "Material softness is bound to an effect parameter".into();
            return;
        };
        let value = value.max(0.0);
        if *current == value {
            return;
        }
        *current = value;
        self.update_material("Changed material softness", material);
    }

    pub fn set_renderer_texture(&mut self, id: RendererId, value: Option<AssetId>) {
        let Some(mut material) = self.renderer_material(id).cloned() else {
            self.status = "Renderer material no longer exists".into();
            return;
        };
        let MaterialProperties::Sprite { texture, .. } = &mut material.properties;
        if *texture == value {
            return;
        }
        *texture = value;
        self.update_material("Changed material texture", material);
    }

    pub fn set_renderer_uv(&mut self, id: RendererId, component: u8, value: f32) {
        let Some(mut material) = self.renderer_material(id).cloned() else {
            self.status = "Renderer material no longer exists".into();
            return;
        };
        let MaterialProperties::Sprite { uv, .. } = &mut material.properties;
        let value = value.clamp(0.0, 1.0);
        match component {
            0 => uv.min[0] = value.min(uv.max[0]),
            1 => uv.min[1] = value.min(uv.max[1]),
            2 => uv.max[0] = value.max(uv.min[0]),
            3 => uv.max[1] = value.max(uv.min[1]),
            _ => {
                self.status = "Unknown UV component".into();
                return;
            }
        }
        self.update_material("Changed material UV bounds", material);
    }

    fn renderer_material(&self, id: RendererId) -> Option<&MaterialDefinition> {
        let renderer = self
            .selected_layer()
            .renderers
            .iter()
            .find(|renderer| renderer.id == id)?;
        self.effect
            .materials
            .iter()
            .find(|material| material.id == renderer.material)
    }

    fn update_material(&mut self, label: &str, material: MaterialDefinition) {
        self.execute(
            label,
            EffectCommand::SetMaterial {
                id: material.id,
                material,
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

    pub fn set_emitter_timing(
        &mut self,
        id: EmitterId,
        start_time: f32,
        duration: f32,
        label: impl Into<String>,
    ) -> bool {
        self.execute(
            label,
            EffectCommand::SetEmitterTiming {
                id,
                start_time,
                duration,
            },
            false,
        )
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

    fn clamp_clock(&mut self) {
        self.clock
            .seek_frame(self.clock.frame(), self.effect.duration);
    }

    fn invalidate_effect_checkpoints(&mut self) {
        self.effect_revision = self.effect_revision.wrapping_add(1);
        self.checkpoints.clear();
        self.last_seek = direct_seek_plan(self.frame());
    }

    fn refresh_preview(&mut self) {
        if self.solo_emitter.is_some_and(|solo| {
            !self
                .effect
                .emitters
                .iter()
                .any(|emitter| emitter.id == solo)
        }) {
            self.solo_emitter = None;
        }
        match compile_preview_with_solo(&self.effect, self.preview_seed, self.solo_emitter) {
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

    fn update_dirty_state(&mut self) {
        self.dirty = self
            .saved_effect
            .as_ref()
            .is_none_or(|saved| saved != &self.effect);
    }
}

fn compile_preview(effect: &EffectAsset, seed: u64) -> Result<EffectInstance, CompileError> {
    let compiled = EffectCompiler::default().compile(effect)?;
    Ok(EffectInstance::with_seed(Arc::new(compiled), seed))
}

fn compile_preview_with_solo(
    effect: &EffectAsset,
    seed: u64,
    solo_emitter: Option<EmitterId>,
) -> Result<EffectInstance, CompileError> {
    let Some(solo_emitter) = solo_emitter else {
        return compile_preview(effect, seed);
    };
    let mut preview_effect = effect.clone();
    for emitter in &mut preview_effect.emitters {
        emitter.enabled &= emitter.id == solo_emitter;
    }
    // Effect clips are top-level tracks alongside local emitters. Soloing a local emitter must
    // therefore suppress referenced effects as well as the other local emitters.
    preview_effect.effect_clips.clear();
    compile_preview(&preview_effect, seed)
}

fn direct_seek_plan(frame: u64) -> SeekPlan {
    SeekPlan {
        target_frame: frame,
        origin: SeekOrigin::Direct,
        replay_ticks: 0,
    }
}

fn seek_origin_label(origin: SeekOrigin) -> &'static str {
    match origin {
        SeekOrigin::Direct => "DIRECT",
        SeekOrigin::Current => "FORWARD REPLAY",
        SeekOrigin::Checkpoint { .. } => "CHECKPOINT RESTORE",
        SeekOrigin::Restart => "RESTART REPLAY",
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
    fn interaction_preview_does_not_mutate_document_or_history() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let original = session.effect.clone();
        let emitter = session.selected_layer().id;
        let module = session
            .selected_layer()
            .module_by_type(aestra_bevy::MODULE_SHAPE)
            .unwrap()
            .id;

        let command = EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter: "shape".into(),
            value: Value::Shape(aestra_bevy::EmitterShape::Circle { radius: 42.0 }),
        };
        assert!(
            session
                .preview_interaction(EffectTransaction::single("Preview radius", command.clone(),))
        );

        assert_eq!(session.effect, original);
        assert!(!session.can_undo());
        assert!(session.pending_change.is_none());

        assert!(session.execute("Adjusted spawn shape", command, true));
        assert!(session.can_undo());
        session.undo();
        assert_eq!(session.effect, original);
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
    fn dirty_state_tracks_the_saved_document_across_undo_and_redo() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let module = session.effect.emitters[0]
            .module_by_type(aestra_bevy::MODULE_EMISSION)
            .unwrap()
            .id;
        let original = session.effect.emitters[0].spawn_rate();
        session.set_module_parameter(module, "spawn_rate", Value::Scalar(77.0));
        assert!(session.dirty);

        session.undo();
        assert_eq!(session.effect.emitters[0].spawn_rate(), original);
        assert!(!session.dirty);

        session.redo();
        assert!(session.dirty);
        let path = std::env::temp_dir().join(format!(
            "aestra-dirty-checkpoint-{}.aestra.ron",
            std::process::id()
        ));
        session.save_as(&path).unwrap();
        assert!(!session.dirty);

        session.undo();
        assert!(session.dirty);
        session.redo();
        assert!(!session.dirty);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn recovered_document_preserves_source_identity_and_dirty_state() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let path = std::env::temp_dir().join(format!(
            "aestra-recovery-source-{}.aestra.ron",
            std::process::id()
        ));
        session.effect.save_ron(&path).unwrap();
        let mut recovered = session.effect.clone();
        recovered.name = "Recovered name".into();

        session.restore_recovery(recovered.clone(), Some(path.clone()));

        assert_eq!(session.effect, recovered);
        assert_eq!(session.source_path.as_deref(), Some(path.as_path()));
        assert!(session.dirty);
        assert!(!session.can_undo());
        std::fs::remove_file(path).unwrap();
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
    fn blank_effect_can_be_authored_reopened_and_compiled_without_ron_edits() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        session.new_effect();
        let blank = session.effect.clone();
        let primary = session.selected_layer().id;

        assert!(session.set_effect_name("Impact Burst"));
        assert!(session.set_effect_looping(false));
        assert!(session.set_selected_emitter_name("Core"));
        assert!(session.set_selected_emitter_capacity(256));
        session.add_layer();
        let secondary = session.selected_layer().id;
        assert!(session.set_selected_emitter_name("Sparks"));
        assert!(session.set_selected_emitter_capacity(512));
        assert!(session.set_emitter_timing(secondary, 0.25, 1.5, "Timed Sparks"));
        assert!(
            session
                .add_event_link(EventTrigger::OnDeath, primary)
                .is_ok()
        );

        let authored = session.effect.clone();
        assert_eq!(authored.emitters.len(), 2);
        assert_eq!(authored.events.len(), 1);
        while session.can_undo() {
            session.undo();
        }
        assert_eq!(session.effect, blank);
        while session.can_redo() {
            session.redo();
        }
        assert_eq!(session.effect, authored);

        let path = std::env::temp_dir().join(format!(
            "aestra-blank-authoring-{}.aestra.ron",
            std::process::id()
        ));
        session.save_as(&path).unwrap();
        let loaded = EffectAsset::load_ron(&path).unwrap();
        let compiled = EffectCompiler::default().compile(&loaded).unwrap();
        assert_eq!(loaded.name, "Impact Burst");
        assert!(!loaded.looping);
        assert_eq!(loaded.events[0].source, secondary);
        assert_eq!(loaded.events[0].target, primary);
        assert_eq!(compiled.emitters.len(), 2);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn event_links_reject_self_targets_and_duplicates() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        session.new_effect();
        let first = session.selected_layer().id;
        assert_eq!(
            session.add_event_link(EventTrigger::OnSpawn, first),
            Err(EventLinkError::SameEmitter)
        );
        session.add_layer();
        let second = session.selected_layer().id;
        assert!(session.add_event_link(EventTrigger::OnSpawn, first).is_ok());
        assert_eq!(
            session.add_event_link(EventTrigger::OnSpawn, first),
            Err(EventLinkError::Duplicate)
        );
        assert_eq!(session.effect.events[0].source, second);
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
        session.execute(
            "Deleted emitter layer",
            EffectCommand::RemoveEmitter { id: added },
            true,
        );
        assert_eq!(session.effect.emitters.len(), 1);
        session.undo();
        assert_eq!(session.effect.emitters.len(), 2);
        assert_eq!(session.effect.emitters[1].id, added);
    }

    #[test]
    fn renderer_texture_assignment_is_compiled_and_undoable() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/ember_sigil.aestra.ron"),
            "ember_sigil.aestra.ron",
        );
        let renderer = session.effect.emitters[0].renderers[0].id;
        let material = session.effect.emitters[0].renderers[0].material;
        let MaterialProperties::Sprite { texture, .. } = &session
            .effect
            .materials
            .iter()
            .find(|candidate| candidate.id == material)
            .unwrap()
            .properties;
        let texture_asset = texture.expect("ember material should use a texture");

        session.set_renderer_texture(renderer, None);
        let MaterialProperties::Sprite { texture, .. } = &session
            .effect
            .materials
            .iter()
            .find(|candidate| candidate.id == material)
            .unwrap()
            .properties;
        assert!(texture.is_none());

        session.set_renderer_texture(renderer, Some(texture_asset));
        let compiled = session.preview.as_ref().unwrap().effect();
        assert!(compiled.material(material).unwrap().texture.is_some());

        session.undo();
        let MaterialProperties::Sprite { texture, .. } = &session
            .effect
            .materials
            .iter()
            .find(|candidate| candidate.id == material)
            .unwrap()
            .properties;
        assert!(texture.is_none());
        session.undo();
        let MaterialProperties::Sprite { texture, .. } = &session
            .effect
            .materials
            .iter()
            .find(|candidate| candidate.id == material)
            .unwrap()
            .properties;
        assert!(texture.is_some());

        session.set_renderer_uv(renderer, 0, 0.1);
        let MaterialProperties::Sprite { uv, .. } = &session
            .effect
            .materials
            .iter()
            .find(|candidate| candidate.id == material)
            .unwrap()
            .properties;
        assert_eq!(uv.min[0], 0.1);
        session.undo();
        let MaterialProperties::Sprite { uv, .. } = &session
            .effect
            .materials
            .iter()
            .find(|candidate| candidate.id == material)
            .unwrap()
            .properties;
        assert_eq!(uv.min[0], 0.0);
    }

    #[test]
    fn material_creation_and_assignment_are_undoable() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let renderer = session.effect.emitters[0].renderers[0].id;
        let original_material = session.effect.emitters[0].renderers[0].material;
        let original_count = session.effect.materials.len();

        session.add_sprite_material();
        assert_eq!(session.effect.materials.len(), original_count + 1);
        let added_material = session.effect.materials.last().unwrap().id;
        session.set_renderer_material(renderer, added_material);
        assert_ne!(
            session.effect.emitters[0].renderers[0].material,
            original_material
        );

        session.undo();
        assert_eq!(
            session.effect.emitters[0].renderers[0].material,
            original_material
        );
        session.undo();
        assert_eq!(session.effect.materials.len(), original_count);
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

        let emitter = session.selected_layer().id;
        session.execute(
            "Deleted module",
            EffectCommand::RemoveModule {
                emitter,
                module: original,
            },
            true,
        );
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
    fn transaction_preview_applies_as_one_undoable_edit() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let emitter = session.effect.emitters[0].id;
        let module = session.effect.emitters[0]
            .module_by_type(aestra_bevy::MODULE_EMISSION)
            .unwrap()
            .id;
        let original = session.effect.emitters[0].spawn_rate();
        let transaction = EffectTransaction::new(
            "Tune emission",
            vec![
                EffectCommand::SetModuleParameter {
                    emitter,
                    module,
                    parameter: "spawn_rate".into(),
                    value: Value::Scalar(91.0),
                },
                EffectCommand::SetEffectLooping { looping: false },
            ],
        );

        assert!(session.preview_transaction(transaction));
        assert_eq!(session.effect.emitters[0].spawn_rate(), original);
        assert_eq!(preview_spawn_rate(&session), 91.0);
        assert_eq!(
            session
                .pending_change
                .as_ref()
                .unwrap()
                .preview
                .diff()
                .changes
                .len(),
            2
        );

        assert!(session.apply_pending_change());
        assert!(session.pending_change.is_none());
        assert_eq!(session.effect.emitters[0].spawn_rate(), 91.0);
        session.undo();
        assert_eq!(session.effect.emitters[0].spawn_rate(), original);
        assert!(session.effect.looping);
    }

    #[test]
    fn discarded_and_invalid_previews_never_change_the_effect() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let before = session.effect.clone();
        assert!(session.preview_transaction(EffectTransaction::single(
            "Rename effect",
            EffectCommand::SetEffectName {
                name: "Candidate".into(),
            },
        )));
        assert!(session.discard_pending_change());
        assert_eq!(session.effect, before);

        let emitter = session.effect.emitters[0].id;
        let required_module = session.effect.emitters[0]
            .module_by_type(aestra_bevy::MODULE_EMISSION)
            .unwrap()
            .id;
        assert!(session.preview_transaction(EffectTransaction::single(
            "Delete required module",
            EffectCommand::RemoveModule {
                emitter,
                module: required_module,
            },
        )));
        let pending = session.pending_change.as_ref().unwrap();
        assert!(!pending.can_apply);
        assert!(!pending.diagnostics.is_valid());
        assert!(!session.apply_pending_change());
        assert_eq!(session.effect, before);
    }

    #[test]
    fn editor_frame_controls_are_reproducible() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        session.restart();
        for _ in 0..120 {
            session.advance_playback(1.0 / 120.0);
        }
        assert_eq!(session.frame(), 60);
        assert_eq!(session.time(), 1.0);
        let first = preview_samples(&mut session);

        session.restart();
        session.seek_time(1.0);
        assert_eq!(session.frame(), 60);
        assert_eq!(preview_samples(&mut session), first);

        session.step_frame(-1);
        assert_eq!(session.frame(), 59);
        assert!(!session.playing);
    }

    #[test]
    fn stateful_scrubbing_restores_checkpoint_and_replays() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        set_seek_mode(&mut session, SimulationSeekMode::CheckpointRestore);
        session.seek_time(1.0);
        assert_eq!(session.frame(), 60);
        assert_eq!(session.checkpoints.len(), 2);

        session.seek_time(0.75);
        assert_eq!(session.frame(), 45);
        assert_eq!(
            session.last_seek.origin,
            SeekOrigin::Checkpoint { frame: 30 }
        );
        let restored = preview_samples(&mut session);

        session.restart_for_replay();
        session.replay_ticks(45);
        assert_eq!(preview_samples(&mut session), restored);
    }

    #[test]
    fn effect_changes_invalidate_editor_checkpoints() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        set_seek_mode(&mut session, SimulationSeekMode::CheckpointRestore);
        session.seek_time(1.0);
        assert!(!session.checkpoints.is_empty());
        let module = session.effect.emitters[0]
            .module_by_type(aestra_bevy::MODULE_EMISSION)
            .unwrap()
            .id;
        session.set_module_parameter(module, "spawn_rate", Value::Scalar(25.0));
        assert!(session.checkpoints.is_empty());
    }

    #[test]
    fn snapshotless_stateful_seek_uses_restart_replay_fallback() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        set_seek_mode(&mut session, SimulationSeekMode::RestartReplay);
        session.seek_time(1.0);
        session.seek_time(0.5);
        assert_eq!(session.last_seek.origin, SeekOrigin::Restart);
        assert_eq!(session.last_seek.replay_ticks, 30);
    }

    #[test]
    fn selection_uses_semantic_emitter_id() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let id = session.effect.emitters[2].id;
        assert!(session.select_emitter(id));
        assert_eq!(session.selection.emitter(&session.effect), Some(id));
    }

    #[test]
    fn flipbook_authoring_is_compiled_and_undoable() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/ember_sigil.aestra.ron"),
            "ember.ron",
        );
        session.add_grid_flipbook();
        assert_eq!(session.effect.flipbooks.len(), 1);
        session.add_flipbook_renderer();
        let renderer = session.effect.emitters[0].renderers.last().unwrap().id;
        assert!(matches!(
            session.effect.emitters[0]
                .renderers
                .last()
                .unwrap()
                .properties,
            RendererProperties::Flipbook { .. }
        ));
        session.set_flipbook_frame_rate(renderer, 15.0);
        assert_eq!(session.effect.flipbooks[0].frame_rate, 15.0);
        assert!(session.preview.is_some());
        session.undo();
        assert_eq!(session.effect.flipbooks[0].frame_rate, 12.0);
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
            aestra_runtime::Instruction::Emit {
                spawn_rate: aestra_runtime::ScalarSource::Constant(spawn_rate),
                ..
            } => *spawn_rate
                .constant_value()
                .expect("editor-authored spawn rate is constant"),
            _ => panic!("first emitter instruction must be emission"),
        }
    }

    fn preview_samples(session: &mut EditorSession) -> Vec<aestra_bevy::ParticleSample> {
        let time = session.time();
        let preview = session.preview.as_mut().unwrap();
        preview.seek(time);
        let mut samples = Vec::new();
        preview.evaluate(&mut samples);
        samples
    }

    fn set_seek_mode(session: &mut EditorSession, mode: SimulationSeekMode) {
        let mut compiled = EffectCompiler::default().compile(&session.effect).unwrap();
        compiled.seek_mode = mode;
        session.preview = Some(EffectInstance::with_seed(
            Arc::new(compiled),
            session.preview_seed,
        ));
        session.clock.restart();
        session.checkpoints.clear();
    }
}
