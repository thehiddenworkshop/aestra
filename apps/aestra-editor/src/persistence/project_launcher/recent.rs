//! Native list pilot: presentation and project-opening policy stay in the launcher.
use super::*;
use bevy::{
    feathers::controls::{FeathersListRow, FeathersListView},
    scene::Ready,
    ui::{InteractionDisabled, Selected},
    ui_widgets::{ListItem, ReselectListRow, ScrollArea, ValueChange},
};

#[derive(Component)]
pub(super) struct RecentProjectPath(pub(super) PathBuf);

#[derive(Component, Default, Clone)]
pub(super) struct RecentProjectList;

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    settings: &EditorSettings,
    localizer: &Localizer,
) {
    let rows: Box<dyn SceneList> = if settings.general.recent_projects.is_empty() {
        let text = localizer.text("project-launcher-no-recent");
        Box::new(bsn! {
            Node { width: percent(100), min_height: px(125), align_items: AlignItems::Center, justify_content: JustifyContent::Center }
            BackgroundColor(theme::PANEL_DARK)
            Children [ Text(text) TextColor(theme::TEXT_FAINT) Pickable::IGNORE ]
        })
    } else {
        Box::new(
            settings
                .general
                .recent_projects
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    row(
                        index,
                        path.clone(),
                        settings.general.active_project.as_ref() == Some(path),
                        localizer,
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    let available = settings
        .general
        .recent_projects
        .iter()
        .any(|path| path.is_dir());
    let label = localizer.text("project-launcher-recent");
    parent.spawn_empty().apply_scene(bsn! {
        @FeathersListView { @rows: rows }
        RecentProjectList
        AccessibleLabel(label)
        Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_direction: FlexDirection::Column, padding: UiRect::right(px(14)) }
        // Commit requests open a project and destroy this overlay. Do not queue
        // a second retained-selection update against rows which may be gone.
        on(select)
        on(reselect)
        on(move |ready: On<Ready>, children: Query<&Children>, mut areas: Query<&mut Node, With<ScrollArea>>, mut commands: Commands| {
            if !available {
                commands.entity(ready.entity).insert((InteractionDisabled, bevy::input_focus::tab_navigation::TabIndex(-1)));
            }
            for area in children.iter_descendants(ready.entity) {
                if let Ok(mut viewport) = areas.get_mut(area) {
                    viewport.min_width = px(0);
                    viewport.min_height = px(0);
                    viewport.flex_grow = 1.0;
                    commands.entity(area).insert(crate::feathers::scroll::PersistedScroll(crate::ScrollMemoryKey::RecentProjects));
                }
            }
        })
    });
}

fn row(index: usize, path: PathBuf, current: bool, localizer: &Localizer) -> Box<dyn SceneList> {
    let available = path.is_dir();
    let missing_label = localizer.text("project-launcher-missing");
    let current_label = localizer.text("project-launcher-current");
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let accessible = format!(
        "{name}, {}{}",
        new_project::display_path(&path),
        if available {
            String::new()
        } else {
            format!(", {missing_label}")
        }
    );
    let border = if current {
        theme::ACCENT_DIM
    } else {
        theme::BORDER_BRIGHT
    };
    bsn! {
        @FeathersListRow
        Node {
            width: percent(100), min_width: px(0), min_height: px(72), flex_shrink: 0.0,
            align_items: AlignItems::Center, column_gap: px(14), margin: UiRect::bottom(px(8)),
            padding: UiRect::all(px(12)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(5)),
        }
        BorderColor::all(border)
        AccessibleLabel(accessible)
        on(move |ready: On<Ready>, mut commands: Commands| {
            let mut row = commands.entity(ready.entity);
            row.insert(RecentProjectPath(path.clone()));
            if available {
                row.insert(Choice::Recent(index));
                if current { row.insert(Selected); }
            } else {
                // Missing rows are readable list entries, but must not become a
                // dead keyboard stop: native ListBox navigates all ListItems.
                row.insert(InteractionDisabled).remove::<ListItem>();
            }
            row.with_children(|content| super::recent_project_content(content, &path, current, available, &missing_label, &current_label));
        })
    }.into()
}

type EnabledList = (With<RecentProjectList>, Without<InteractionDisabled>);
type EnabledProject = (
    With<RecentProjectPath>,
    With<ListItem>,
    Without<InteractionDisabled>,
);

fn select(
    event: On<ValueChange<Entity>>,
    lists: Query<(), EnabledList>,
    rows: Query<(), EnabledProject>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    if lists.contains(event.source)
        && rows.contains(event.value)
        && event.is_final
        && parents
            .iter_ancestors(event.value)
            .any(|parent| parent == event.source)
    {
        commands.trigger(Activate {
            entity: event.value,
        });
    }
}

fn reselect(
    event: On<ReselectListRow>,
    lists: Query<(), EnabledList>,
    rows: Query<(), EnabledProject>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    if lists.contains(event.source)
        && rows.contains(event.row)
        && parents
            .iter_ancestors(event.row)
            .any(|parent| parent == event.source)
    {
        commands.trigger(Activate { entity: event.row });
    }
}

#[cfg(test)]
#[path = "tests/recent.rs"]
mod tests;
