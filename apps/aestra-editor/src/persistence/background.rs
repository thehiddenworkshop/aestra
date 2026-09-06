//! Explicit document operations: select destinations on the UI thread, prepare on the I/O
//! pool, then publish only to the document/project that requested the operation.
use super::*;
use crate::project_content::io::{self, IoGuard};

enum OpenTarget {
    Folder(PathBuf),
    Effect(PathBuf),
}

struct OpenPlan {
    target: OpenTarget,
    navigation: SourceNavigationState,
    restore: Option<SourceNavigationEntry>,
    clip: Option<EffectClipId>,
    emitter: Option<EmitterId>,
}

pub(super) fn queue_open(
    action: DocumentAction,
    commands: &mut Commands,
    session: &mut EditorSession,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    timeline: Option<&TimelineState>,
    navigation: Option<&SourceNavigationState>,
) -> bool {
    if matches!(
        action,
        DocumentAction::New | DocumentAction::Save | DocumentAction::SaveAs | DocumentAction::Exit
    ) {
        return false;
    }
    let plan = match open_plan(action, session, catalog, localizer, timeline, navigation) {
        Ok(Some(plan)) => plan,
        Ok(None) => {
            set_persistence_status(session, localizer, PersistenceStatus::OpenCancelled);
            return true;
        }
        Err(error) => {
            set_persistence_status(session, localizer, PersistenceStatus::OpenFailed(error));
            return true;
        }
    };
    queue_plan(commands, session, settings, catalog, localizer, plan);
    true
}

fn queue_plan(
    commands: &mut Commands,
    session: &EditorSession,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    plan: OpenPlan,
) {
    let guard = IoGuard::capture(catalog, session);
    let mut prepared_session = session.fork_for_io();
    let mut prepared_catalog = catalog.clone();
    prepared_catalog.material_drafts = default();
    let settings = settings.clone();
    let locale = localizer.locale();
    io::enqueue(commands, guard.clone(), move || {
        let localizer = Localizer::new(locale).expect("supported locale");
        let opened = match &plan.target {
            OpenTarget::Folder(folder) => match crate::project::open_folder(
                &mut prepared_session,
                &mut prepared_catalog,
                folder,
            ) {
                Ok(()) => {
                    prepared_session.status = localizer.text("project-opened");
                    true
                }
                Err(error) => {
                    set_persistence_status(
                        &mut prepared_session,
                        &localizer,
                        PersistenceStatus::OpenFailed(error),
                    );
                    false
                }
            },
            OpenTarget::Effect(path) => {
                prepared_catalog.refresh();
                open_effect_in_project(
                    &mut prepared_session,
                    path,
                    &settings,
                    &mut prepared_catalog,
                    &localizer,
                )
            }
        };
        // Include migrations and verify the loaded document still agrees with discovery.
        let opened = opened
            && (|| {
                prepared_catalog.refresh();
                if let OpenTarget::Effect(_) = plan.target {
                    if prepared_catalog.cached_effect(prepared_session.effect.id.into())?
                        != prepared_session.effect
                    {
                        return Err(
                            "The source changed while opening it; retry the open.".to_owned()
                        );
                    }
                    prepared_catalog.prepare_preview(&prepared_session.effect)?;
                }
                if !prepared_catalog.snapshot_is_current() {
                    return Err("Project sources changed while opening; retry the open.".to_owned());
                }
                Ok(())
            })()
            .map_err(|error| {
                set_persistence_status(
                    &mut prepared_session,
                    &localizer,
                    PersistenceStatus::OpenFailed(error),
                )
            })
            .is_ok();
        io::completion(move |world| {
            if !guard.matches(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            ) {
                io::set_status(world, "project-operation-open-stale");
                return;
            }
            if !opened {
                world.resource_mut::<EditorSession>().status = prepared_session.status;
                return;
            }
            prepared_session.playing = settings.preview.play_on_open;
            prepared_session.ui_revision = world.resource::<EditorSession>().ui_revision + 1;
            world.insert_resource(prepared_session);
            // Successful document switches deliberately discard the approved old drafts.
            world.resource_mut::<ProjectEffectCatalog>().material_drafts = default();
            io::publish_catalog(world, prepared_catalog);
            world.insert_resource(plan.navigation);
            if let Some(mut workspace) = world.get_resource_mut::<CurvesState>() {
                workspace.clear();
            }
            let duration = world.resource::<EditorSession>().playback_duration();
            if let Some(mut timeline) = world.get_resource_mut::<TimelineState>() {
                *timeline = TimelineState::framed(duration);
            }
            if let Some(entry) = plan.restore {
                if let Some(mut timeline) = world.get_resource_mut::<TimelineState>() {
                    timeline.restore_navigation(entry.timeline, duration);
                }
                let mut session = world.resource_mut::<EditorSession>();
                session.selection.primary = entry.selection;
                let effect = session.effect.clone();
                session.selection.repair(&effect);
                session.seek_time(entry.playhead_time);
                session.playing = entry.playing;
            }
            if let Some(clip) = plan.clip {
                world
                    .resource_mut::<EditorSession>()
                    .select_effect_clip(clip);
            }
            if let Some(emitter) = plan.emitter {
                world
                    .resource_mut::<EditorSession>()
                    .select_emitter(emitter);
                if let Some(mut timeline) = world.get_resource_mut::<TimelineState>() {
                    timeline.reveal_emitter(emitter);
                }
            }
        })
    });
}

fn open_plan(
    action: DocumentAction,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
    timeline: Option<&TimelineState>,
    navigation: Option<&SourceNavigationState>,
) -> Result<Option<OpenPlan>, String> {
    let mut plan = OpenPlan {
        target: OpenTarget::Effect(PathBuf::new()),
        navigation: navigation.cloned().unwrap_or_default(),
        restore: None,
        clip: None,
        emitter: None,
    };
    match action {
        DocumentAction::OpenProject => {
            let Some(folder) = FileDialog::new()
                .set_title(localizer.text("project-open-title"))
                .set_directory(catalog.root())
                .pick_folder()
            else {
                return Ok(None);
            };
            plan.target = OpenTarget::Folder(folder);
            plan.navigation.clear();
        }
        DocumentAction::Open => {
            let directory = session
                .source_path
                .as_deref()
                .and_then(Path::parent)
                .unwrap_or(catalog.effect_root());
            let Some(path) = FileDialog::new()
                .add_filter(localizer.text("persistence-file-filter-effect"), &["ron"])
                .set_directory(directory)
                .pick_file()
            else {
                return Ok(None);
            };
            plan.target = OpenTarget::Effect(path);
            plan.navigation.clear();
        }
        DocumentAction::OpenCatalog(id) | DocumentAction::OpenCatalogClip(id, _) => {
            plan.target = OpenTarget::Effect(
                catalog
                    .openable_path(id)
                    .ok_or("Referenced source is unavailable")?
                    .to_owned(),
            );
            plan.navigation.clear();
            if let DocumentAction::OpenCatalogClip(_, clip) = action {
                plan.clip = Some(clip);
            }
        }
        DocumentAction::OpenSource(id) | DocumentAction::OpenSourceEmitter(id, _) => {
            if id.id == session.effect.id || plan.navigation.contains(id) {
                return Err("Source is already in the breadcrumb".into());
            }
            let timeline = timeline.ok_or("Timeline is unavailable")?;
            let return_path = session
                .source_path
                .clone()
                .ok_or("Save the current effect before opening a referenced source")?;
            plan.target = OpenTarget::Effect(
                catalog
                    .openable_path(id)
                    .ok_or("Referenced source is unavailable")?
                    .to_owned(),
            );
            plan.navigation
                .back
                .push(source_navigation_entry(session, timeline, return_path));
            plan.navigation.forward.clear();
            if let DocumentAction::OpenSourceEmitter(_, emitter) = action {
                plan.emitter = Some(emitter);
            }
        }
        DocumentAction::BackToSource | DocumentAction::NavigateSourceAncestor(_) => {
            let Some(depth) = (match action {
                DocumentAction::NavigateSourceAncestor(depth) => Some(depth),
                _ => plan.navigation.back.len().checked_sub(1),
            }) else {
                return Ok(None);
            };
            let Some(entry) = plan.navigation.back.get(depth).cloned() else {
                return Ok(None);
            };
            let current = current_source_navigation_entry(
                session,
                timeline.ok_or("Timeline is unavailable")?,
            )
            .ok_or("Current source has no location")?;
            plan.target = OpenTarget::Effect(entry.path.clone());
            let traversed = plan.navigation.back.split_off(depth + 1);
            plan.navigation.back.truncate(depth);
            plan.navigation.forward.push(current);
            plan.navigation.forward.extend(traversed.into_iter().rev());
            plan.restore = Some(entry);
        }
        DocumentAction::ForwardToSource => {
            let Some(entry) = plan.navigation.forward.pop() else {
                return Ok(None);
            };
            plan.navigation.back.push(
                current_source_navigation_entry(
                    session,
                    timeline.ok_or("Timeline is unavailable")?,
                )
                .ok_or("Current source has no location")?,
            );
            plan.target = OpenTarget::Effect(entry.path.clone());
            plan.restore = Some(entry);
        }
        _ => return Ok(None),
    }
    Ok(Some(plan))
}

pub(super) fn queue_save(
    commands: &mut Commands,
    session: &mut EditorSession,
    catalog: &ProjectEffectCatalog,
    save_as: bool,
    continuation: Option<DocumentAction>,
    localizer: &Localizer,
) {
    let destination = if save_as || session.source_path.is_none() {
        let directory = session
            .source_path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(catalog.effect_root());
        let Some(path) = FileDialog::new()
            .add_filter(localizer.text("persistence-file-filter-effect"), &["ron"])
            .set_file_name(format!("{}.aestra.ron", session.effect.id))
            .set_directory(directory)
            .save_file()
        else {
            set_persistence_status(session, localizer, PersistenceStatus::SaveCancelled);
            return;
        };
        Some(path)
    } else {
        None
    };
    let guard = IoGuard::capture(catalog, session);
    let mut saved = session.fork_for_io();
    let mut prepared = catalog.clone();
    let before = catalog.material_drafts.clone();
    let locale = localizer.locale();
    io::enqueue(commands, guard.clone(), move || {
        let localizer = Localizer::new(locale).expect("supported locale");
        let success = if let Some(path) = destination {
            let result = prepared
                .material_drafts
                .preflight()
                .and_then(|_| saved.save_as(&path).map_err(|e| e.to_string()));
            match result {
                Ok(()) => finish_material_save(
                    &mut saved,
                    &mut prepared,
                    &localizer,
                    path.display().to_string(),
                ),
                Err(error) => {
                    set_persistence_status(
                        &mut saved,
                        &localizer,
                        PersistenceStatus::SaveFailed(error),
                    );
                    false
                }
            }
        } else {
            save_session(&mut saved, false, &localizer, &mut prepared)
        };
        let receipts = crate::material_drafts::MaterialDrafts::saved_baselines(
            &before,
            &prepared.material_drafts,
        );
        // An effect write can precede a material failure. Always reconcile its post-write view.
        prepared.refresh();
        io::completion(move |world| {
            if !guard.same_document(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            ) {
                io::set_status(world, "project-operation-save-stale");
                return;
            }
            world
                .resource_mut::<ProjectEffectCatalog>()
                .material_drafts
                .accept_saved_baselines(&before, receipts);
            let drafts = world
                .resource::<ProjectEffectCatalog>()
                .material_drafts
                .clone();
            {
                let mut session = world.resource_mut::<EditorSession>();
                session.accept_io_save(&saved);
                session.set_material_drafts(drafts);
            }
            io::publish_catalog(world, prepared);
            if success
                && let Some(action) = continuation
                && world.resource::<DocumentProtectionState>().pending == Some(action)
            {
                world.resource_mut::<DocumentProtectionState>().pending = None;
                // Re-enter the regular guard: edits made during the save need confirmation again.
                world.trigger(action);
            }
        })
    });
}

#[cfg(test)]
mod tests;
