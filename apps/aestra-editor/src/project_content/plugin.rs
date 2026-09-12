//! Project services live independently of any browsing panel.
use super::*;

pub(crate) struct EditorProjectContentPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProjectContentSet {
    Input,
}

impl Plugin for EditorProjectContentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorProjectContent>()
            .init_resource::<io::ProjectIoTasks>()
            .init_resource::<ProjectEffectWatchState>()
            .init_resource::<aestra_bevy_render::AestraTextureRoot>()
            .add_systems(
                Update,
                (io::poll, poll_project_effect_catalog.run_if(io::idle))
                    .chain()
                    .in_set(ProjectContentSet::Input),
            )
            .add_systems(
                Update,
                sync_project_texture_root
                    .after(PersistenceSet::Actions)
                    .before(aestra_bevy_render::AestraRenderSet::Prepare),
            );
    }
}

fn sync_project_texture_root(
    catalog: Res<EditorProjectContent>,
    mut textures: ResMut<aestra_bevy_render::AestraTextureRoot>,
) {
    if textures.0.as_deref() != Some(catalog.root()) {
        textures.0 = Some(catalog.root().to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource)]
    struct Completed;

    #[test]
    fn project_plugin_polls_io_completions_without_a_library_panel() {
        let root = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.insert_resource(EditorProjectContent::scan(root.path()))
            .insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins(EditorProjectContentPlugin);
        let guard = io::IoGuard::capture(
            app.world().resource::<EditorProjectContent>(),
            app.world().resource::<EditorSession>(),
        );
        io::enqueue(&mut app.world_mut().commands(), guard, || {
            io::completion(|world| {
                world.insert_resource(Completed);
            })
        });
        app.world_mut().flush();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.world().contains_resource::<Completed>() {
            app.update();
            assert!(
                std::time::Instant::now() < deadline,
                "project service did not publish the I/O completion"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn project_services_follow_root_switches_without_library_or_browser() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let mut app = App::new();
        let session = crate::test_support::session_with_timing_slack();
        let original = session.effect.clone();
        app.insert_resource(EditorProjectContent::scan(first.path()))
            .insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins(EditorProjectContentPlugin);
        app.update();
        assert_eq!(
            app.world()
                .resource::<aestra_bevy_render::AestraTextureRoot>()
                .0
                .as_deref(),
            Some(first.path())
        );
        app.insert_resource(EditorProjectContent::scan(second.path()));
        app.update();
        let catalog = app.world().resource::<EditorProjectContent>();
        assert_eq!(
            app.world().resource::<ProjectEffectWatchState>().version(),
            catalog.content_revision()
        );
        assert_eq!(
            app.world()
                .resource::<aestra_bevy_render::AestraTextureRoot>()
                .0
                .as_deref(),
            Some(second.path())
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
        assert!(app.world().contains_resource::<io::ProjectIoTasks>());
        assert!(
            !app.world()
                .contains_resource::<crate::asset_browser::AssetBrowserState>()
        );
    }
}
