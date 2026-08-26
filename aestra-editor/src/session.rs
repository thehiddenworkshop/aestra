use aestra_bevy::{EffectAsset, Emitter};
use bevy::prelude::Resource;
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct EditorSnapshot {
    effect: EffectAsset,
    selected_layer: usize,
    time: f32,
}

struct HistoryEntry {
    label: String,
    before: EditorSnapshot,
    after: EditorSnapshot,
}

#[derive(Default)]
struct CommandHistory {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

#[derive(Resource)]
pub(crate) struct EditorSession {
    pub effect: EffectAsset,
    pub source_path: Option<PathBuf>,
    pub selected_layer: usize,
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
        Self {
            effect,
            source_path: Some(path.into()),
            selected_layer: 0,
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
        self.selected_layer = 0;
        self.time = 0.0;
        self.playing = false;
        self.dirty = true;
        self.history = CommandHistory::default();
        self.status = "Created an untitled effect".into();
        self.ui_revision += 1;
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<(), aestra_bevy::AssetError> {
        let path = path.as_ref();
        self.effect = EffectAsset::load_ron(path)?;
        self.source_path = Some(path.to_owned());
        self.selected_layer = 0;
        self.time = 0.0;
        self.playing = false;
        self.dirty = false;
        self.history = CommandHistory::default();
        self.status = format!("Opened {}", path.display());
        self.ui_revision += 1;
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), aestra_bevy::AssetError> {
        let Some(path) = self.source_path.clone() else {
            return Ok(());
        };
        self.save_as(path)
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), aestra_bevy::AssetError> {
        let path = path.as_ref();
        self.effect.save_ron(path)?;
        self.source_path = Some(path.to_owned());
        self.dirty = false;
        self.status = format!("Saved {}", path.display());
        self.ui_revision += 1;
        Ok(())
    }

    pub fn edit(
        &mut self,
        label: impl Into<String>,
        rebuild_ui: bool,
        edit: impl FnOnce(&mut Self),
    ) {
        let label = label.into();
        let before = self.snapshot();
        edit(self);
        self.clamp_selection();
        let after = self.snapshot();
        self.history.undo.push(HistoryEntry {
            label: label.clone(),
            before,
            after,
        });
        self.history.redo.clear();
        self.dirty = true;
        self.status = label;
        if rebuild_ui {
            self.ui_revision += 1;
        }
    }

    pub fn undo(&mut self) {
        let Some(entry) = self.history.undo.pop() else {
            self.status = "Nothing to undo".into();
            return;
        };
        self.restore(&entry.before);
        self.status = format!("Undid {}", entry.label);
        self.history.redo.push(entry);
        self.dirty = true;
        self.ui_revision += 1;
    }

    pub fn redo(&mut self) {
        let Some(entry) = self.history.redo.pop() else {
            self.status = "Nothing to redo".into();
            return;
        };
        self.restore(&entry.after);
        self.status = format!("Redid {}", entry.label);
        self.history.undo.push(entry);
        self.dirty = true;
        self.ui_revision += 1;
    }

    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    pub fn selected_layer(&self) -> &Emitter {
        &self.effect.emitters[self.selected_layer]
    }

    pub fn selected_layer_mut(&mut self) -> &mut Emitter {
        &mut self.effect.emitters[self.selected_layer]
    }

    pub fn add_layer(&mut self) {
        self.edit("Added emitter layer", true, |session| {
            let index = session.effect.emitters.len();
            session.effect.emitters.push(default_layer(index));
            session.selected_layer = index;
        });
    }

    pub fn duplicate_selected_layer(&mut self) {
        self.edit("Duplicated emitter layer", true, |session| {
            let mut layer = session.effect.emitters[session.selected_layer].clone();
            layer.regenerate_ids();
            layer.name = format!("{} Copy", layer.name);
            session
                .effect
                .emitters
                .insert(session.selected_layer + 1, layer);
            session.selected_layer += 1;
        });
    }

    pub fn delete_selected_layer(&mut self) {
        if self.effect.emitters.len() <= 1 {
            self.status = "An effect must keep at least one layer".into();
            return;
        }
        self.edit("Deleted emitter layer", true, |session| {
            session.effect.emitters.remove(session.selected_layer);
        });
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            effect: self.effect.clone(),
            selected_layer: self.selected_layer,
            time: self.time,
        }
    }

    fn restore(&mut self, snapshot: &EditorSnapshot) {
        self.effect = snapshot.effect.clone();
        self.selected_layer = snapshot.selected_layer;
        self.time = snapshot.time;
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        self.selected_layer = self
            .selected_layer
            .min(self.effect.emitters.len().saturating_sub(1));
        self.time = self.time.clamp(0.0, self.effect.duration);
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
    fn edits_support_undo_and_redo() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        let original = session.effect.emitters[0].spawn_rate();
        session.edit("Changed rate", false, |session| {
            *session.selected_layer_mut().spawn_rate_mut() = 77.0;
        });
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
    fn structural_layer_commands_are_reversible() {
        let mut session = EditorSession::from_embedded_sample(
            include_str!("../../assets/effects/prism_bloom.aestra.ron"),
            "sample.ron",
        );
        session.new_effect();
        session.add_layer();
        assert_eq!(session.effect.emitters.len(), 2);
        session.undo();
        assert_eq!(session.effect.emitters.len(), 1);
        session.redo();
        assert_eq!(session.effect.emitters.len(), 2);
        session.delete_selected_layer();
        assert_eq!(session.effect.emitters.len(), 1);
        session.undo();
        assert_eq!(session.effect.emitters.len(), 2);
    }
}
