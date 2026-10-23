//! Real launcher, native list input and application-owned project dispatch.
use super::*;
use crate::feathers::{
    scroll::PersistedScroll,
    text_input::tests::{app_with, focus, tap},
};
use bevy::{
    ecs::system::RunSystemOnce,
    input::keyboard::Key,
    ui_widgets::{ActiveDescendant, Button, ListBox, Scrollbar},
};

#[derive(Resource, Default)]
struct Opened(Vec<DocumentAction>);

struct Fixture {
    app: App,
    first: tempfile::TempDir,
    second: tempfile::TempDir,
    missing: PathBuf,
}

impl Fixture {
    fn new(locale: &str, mode: u8) -> Self {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let missing = first.path().join("missing-project");
        let mut settings = EditorSettings::default();
        settings.general.recent_projects = match mode {
            0 => vec![
                first.path().to_owned(),
                missing.clone(),
                second.path().to_owned(),
            ],
            1 => Vec::new(),
            3 => vec![first.path().to_owned(); 20],
            _ => vec![missing.clone()],
        };
        settings.general.active_project = Some(first.path().to_owned());
        let mut app = app_with(|app| {
            app.insert_resource(settings)
                .insert_resource(Localizer::new(locale).unwrap())
                .insert_resource(ProjectLauncherState::new(false))
                .init_resource::<DocumentProtectionState>()
                .init_resource::<Opened>()
                .add_observer(super::super::open)
                .add_observer(super::super::choose)
                .add_observer(|event: On<DocumentAction>, mut opened: ResMut<Opened>| {
                    opened.0.push(*event)
                })
                .add_systems(Update, super::super::escape);
        });
        app.world_mut().trigger(OpenProjectLauncher);
        app.world_mut().flush();
        for _ in 0..3 {
            app.update();
        }
        Self {
            app,
            first,
            second,
            missing,
        }
    }

    fn list(&mut self) -> Entity {
        self.app
            .world_mut()
            .query_filtered::<Entity, With<RecentProjectList>>()
            .single(self.app.world())
            .unwrap()
    }

    fn row(app: &mut App, path: &Path) -> Entity {
        app.world_mut()
            .query::<(Entity, &RecentProjectPath)>()
            .iter(app.world())
            .find_map(|(e, p)| (p.0 == path).then_some(e))
            .unwrap()
    }

    fn opened(&self) -> &[DocumentAction] {
        &self.app.world().resource::<Opened>().0
    }
}

#[test]
fn real_native_list_preserves_localized_paths_current_and_missing_rows() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::new(locale, 0);
        let list = f.list();
        let current = Fixture::row(&mut f.app, f.first.path());
        let missing = Fixture::row(&mut f.app, &f.missing);
        let w = f.app.world_mut();
        assert!(w.get::<FeathersListView>(list).is_some());
        assert!(w.get::<ListBox>(list).is_some());
        assert_eq!(
            w.get::<bevy::input_focus::tab_navigation::TabIndex>(list)
                .unwrap()
                .0,
            0
        );
        assert!(w.get::<FeathersListRow>(current).is_some());
        assert!(w.get::<Selected>(current).is_some());
        assert!(
            w.get::<Button>(current).is_none(),
            "native rows must not also run button activation"
        );
        assert!(w.get::<ListItem>(missing).is_none());
        assert!(w.get::<InteractionDisabled>(missing).is_some());
        assert!(w.get::<Choice>(missing).is_none());
        assert_eq!(
            w.query_filtered::<Entity, With<ListItem>>().iter(w).count(),
            2
        );
        assert!(
            w.get::<AccessibleLabel>(missing)
                .unwrap()
                .0
                .contains(&w.resource::<Localizer>().text("project-launcher-missing"))
        );
        assert!(
            w.get::<AccessibleLabel>(current)
                .unwrap()
                .0
                .contains(&new_project::display_path(f.first.path()))
        );
        let (_, bar) = w.query::<(Entity, &Scrollbar)>().single(w).unwrap();
        assert_eq!(
            w.get::<PersistedScroll>(bar.target).unwrap().0,
            crate::ScrollMemoryKey::RecentProjects
        );
    }
}

#[test]
fn native_keyboard_skips_missing_navigation_does_not_open_and_commit_opens_once() {
    let mut f = Fixture::new("en-US", 0);
    let list = f.list();
    let second = Fixture::row(&mut f.app, f.second.path());
    focus(&mut f.app, list);
    tap(&mut f.app, KeyCode::ArrowDown, Key::ArrowDown, None);
    assert_eq!(
        f.app.world().get::<ActiveDescendant>(list).unwrap().0,
        Some(second)
    );
    assert!(f.opened().is_empty());
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(f.opened(), &[DocumentAction::OpenRecentProject(2)]);
    assert!(
        f.app
            .world()
            .resource::<ProjectLauncherState>()
            .overlay
            .is_none()
    );
    assert!(
        !f.app
            .world()
            .resource::<DocumentProtectionState>()
            .project_launcher_open
    );
    f.app.world_mut().trigger(OpenProjectLauncher);
    f.app.world_mut().flush();
    let list = f.list();
    focus(&mut f.app, list);
    // Current project is initially selected. Enter takes the native reselect path.
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        f.opened(),
        &[
            DocumentAction::OpenRecentProject(2),
            DocumentAction::OpenRecentProject(0)
        ]
    );
}

#[test]
fn pointer_click_on_row_content_opens_once_without_a_second_button_controller() {
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            events::{Pointer, PointerClick},
            pointer::{Location, PointerButton, PointerId},
        },
        window::{PrimaryWindow, WindowRef},
    };
    let mut f = Fixture::new("en-US", 0);
    let row = Fixture::row(&mut f.app, f.second.path());
    let child = f.app.world().get::<Children>(row).unwrap()[0];
    let window = f
        .app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(f.app.world())
        .unwrap();
    f.app.world_mut().trigger(PointerClick {
        entity: child,
        pointer: Pointer::new(
            PointerId::Mouse,
            Location {
                target: RenderTarget::Window(WindowRef::Entity(window))
                    .normalize(None)
                    .unwrap(),
                position: Vec2::ZERO,
            },
        ),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        duration: std::time::Duration::ZERO,
        count: 1,
    });
    f.app.world_mut().flush();
    assert_eq!(f.opened(), &[DocumentAction::OpenRecentProject(2)]);
    assert!(f.app.world().get_entity(row).is_err());
}

#[test]
fn disabled_foreign_reordered_and_deleted_projects_cannot_dispatch() {
    let mut f = Fixture::new("en-US", 0);
    let list = f.list();
    let row = Fixture::row(&mut f.app, f.first.path());
    let missing = Fixture::row(&mut f.app, &f.missing);
    let foreign = f
        .app
        .world_mut()
        .spawn((
            Choice::Recent(0),
            RecentProjectPath(f.first.path().to_owned()),
        ))
        .id();
    for value in [missing, foreign] {
        f.app.world_mut().trigger(ValueChange {
            source: list,
            value,
            is_final: true,
        });
        f.app.world_mut().flush();
    }
    f.app
        .world_mut()
        .entity_mut(list)
        .insert(InteractionDisabled);
    f.app.world_mut().trigger(Activate { entity: row });
    f.app.world_mut().flush();
    f.app
        .world_mut()
        .entity_mut(list)
        .remove::<InteractionDisabled>();
    f.app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .general
        .recent_projects
        .swap(0, 2);
    f.app.world_mut().trigger(ValueChange {
        source: list,
        value: row,
        is_final: true,
    });
    f.app.world_mut().flush();
    assert!(f.opened().is_empty());
    assert!(
        f.app
            .world()
            .resource::<ProjectLauncherState>()
            .overlay
            .is_some()
    );
    f.app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .general
        .recent_projects
        .swap(0, 2);
    let second = Fixture::row(&mut f.app, f.second.path());
    f.second.close().unwrap();
    f.app.world_mut().trigger(Activate { entity: second });
    f.app.world_mut().flush();
    assert!(f.app.world().resource::<Opened>().0.is_empty());
    assert!(
        f.app
            .world()
            .resource::<ProjectLauncherState>()
            .overlay
            .is_some()
    );
}

#[test]
fn empty_or_missing_only_lists_have_no_dead_tab_stop_and_escape_still_closes() {
    for mode in [1, 2] {
        let mut f = Fixture::new("fr-FR", mode);
        let list = f.list();
        assert!(f.app.world().get::<InteractionDisabled>(list).is_some());
        assert_eq!(
            f.app
                .world()
                .get::<bevy::input_focus::tab_navigation::TabIndex>(list)
                .unwrap()
                .0,
            -1
        );
        assert_eq!(
            f.app
                .world_mut()
                .query_filtered::<Entity, With<ListItem>>()
                .iter(f.app.world())
                .count(),
            0
        );
        if mode == 1 {
            let text = f
                .app
                .world()
                .resource::<Localizer>()
                .text("project-launcher-no-recent");
            assert!(
                f.app
                    .world_mut()
                    .query::<&Text>()
                    .iter(f.app.world())
                    .any(|label| label.0 == text)
            );
        }
        tap(&mut f.app, KeyCode::Escape, Key::Escape, None);
        assert!(
            f.app
                .world()
                .resource::<ProjectLauncherState>()
                .overlay
                .is_none()
        );
        assert!(f.opened().is_empty());
    }
}

#[test]
fn native_list_scrollbar_uses_existing_overflow_only_policy() {
    let mut f = Fixture::new("en-US", 0);
    let (bar, target) = f
        .app
        .world_mut()
        .query::<(Entity, &Scrollbar)>()
        .iter(f.app.world())
        .map(|(e, bar)| (e, bar.target))
        .next()
        .unwrap();
    for (content, display) in [
        (176.0, Display::None),
        (640.0, Display::Flex),
        (200.0, Display::None),
    ] {
        f.app.world_mut().entity_mut(target).insert(ComputedNode {
            size: Vec2::new(400.0, 200.0),
            content_size: Vec2::new(400.0, content),
            ..default()
        });
        f.app
            .world_mut()
            .run_system_once(crate::feathers::scroll::update_scrollbar_visibility)
            .unwrap();
        assert_eq!(f.app.world().get::<Node>(bar).unwrap().display, display);
    }
}

#[test]
fn native_list_layout_constrains_long_history_to_a_scrolling_viewport() {
    let mut f = Fixture::new("en-US", 3);
    f.app.world_mut().spawn(Camera2d);
    for _ in 0..5 {
        f.app.update();
    }
    let (bar, target) = f
        .app
        .world_mut()
        .query::<(Entity, &Scrollbar)>()
        .single(f.app.world())
        .map(|(entity, bar)| (entity, bar.target))
        .unwrap();
    let viewport = f.app.world().get::<ComputedNode>(target).unwrap();
    assert!(
        viewport.size().y > 0.0 && viewport.size().y < 650.0,
        "list viewport must fit the chooser: {viewport:?}"
    );
    assert!(
        viewport.content_size().y > viewport.size().y,
        "long history must overflow its viewport"
    );
    f.app
        .world_mut()
        .run_system_once(crate::feathers::scroll::update_scrollbar_visibility)
        .unwrap();
    assert_eq!(
        f.app.world().get::<Node>(bar).unwrap().display,
        Display::Flex
    );
    let list = f.list();
    focus(&mut f.app, list);
    tap(&mut f.app, KeyCode::End, Key::End, None);
    assert!(f.opened().is_empty());
    assert!(
        f.app.world().get::<ScrollPosition>(target).unwrap().0.y > 0.0,
        "native keyboard navigation must scroll the active row into view"
    );
}
