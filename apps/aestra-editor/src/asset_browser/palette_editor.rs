use crate::feathers::{
    button::spawn_action_button,
    palette_picker::{
        PaletteAction as Action, PaletteColorTarget, PalettePicker, PalettePickerView, Swatch,
    },
};
use crate::project_content::io::{self, IoGuard};
use crate::*;
use aestra_core::PaletteAsset;
use aestra_project::{ProjectContentVersion, ProjectSourceId, content::operations::PaletteSource};
use bevy::{
    asset::{AssetPath, RenderAssetUsages},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::Selected,
    ui_widgets::{Activate, ValueChange},
};

#[derive(Event)]
pub(super) struct Open(pub ProjectSourceId, pub ProjectContentVersion);

#[derive(Resource, Default)]
struct PaletteEditor(Option<Draft>, u64);

struct Draft {
    id: u64,
    source_id: ProjectSourceId,
    source: PaletteSource,
    generation: u64,
    picker: PalettePicker,
    saving: bool,
    error: Option<String>,
    preview: Option<PreviewTexture>,
}

impl Draft {
    fn dirty(&self) -> bool {
        self.picker.colors != self.source.palette().colors()
    }
}

struct PreviewTexture {
    handle: Handle<Image>,
    original: Image,
    shown: Vec<[u8; 3]>,
}

#[derive(Component, Default)]
struct PanelView {
    draft_id: Option<u64>,
    picker: Option<PalettePickerView>,
    initialized: bool,
}

#[derive(Component)]
struct Status;

pub(super) fn register(app: &mut App) {
    app.init_resource::<PaletteEditor>()
        .add_observer(open)
        .add_observer(activate)
        .add_observer(change_color)
        .add_systems(
            Update,
            (sync_preview, sync_view, sync_controls)
                .chain()
                .after(ProjectContentSet::Input)
                .after(DockingSet::Sync),
        );
}

fn open(
    event: On<Open>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    mut editor: ResMut<PaletteEditor>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
    mut images: Option<ResMut<Assets<Image>>>,
) {
    if event.1 != catalog.content_revision() {
        return;
    }
    if let Some(draft) = editor.0.as_mut()
        && draft.generation == event.1.generation
        && (draft.dirty() || draft.saving)
    {
        if event.0 != draft.source_id {
            draft.error = Some(localizer.text("browser-palette-finish-edit"));
        }
        crate::shell::reveal_dock_panel(&mut layout, &mut session, ToolPanel::PaletteEditor);
        return;
    }
    let source = match catalog.content().open_palette_source(event.0) {
        Ok(source) => source,
        Err(error) => {
            session.status = error.to_string();
            session.ui_revision += 1;
            return;
        }
    };
    if let Some(draft) = editor.0.as_mut() {
        restore_preview(draft, images.as_deref_mut());
    }
    editor.1 = editor.1.wrapping_add(1);
    editor.0 = Some(Draft {
        id: editor.1,
        source_id: event.0,
        generation: event.1.generation,
        picker: PalettePicker::new(source.palette().colors().to_vec()),
        source,
        saving: false,
        error: None,
        preview: None,
    });
    crate::shell::reveal_dock_panel(&mut layout, &mut session, ToolPanel::PaletteEditor);
}

pub(crate) fn spawn_palette_editor(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        PanelView::default(),
        super::BrowserSurface,
        crate::feathers::node_graph::FeathersGraphNavigationBlocker,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ));
}

fn sync_view(
    mut panels: Query<(Entity, &mut PanelView)>,
    editor: Res<PaletteEditor>,
    localizer: Res<Localizer>,
    mut commands: Commands,
) {
    let draft_id = editor.0.as_ref().map(|draft| draft.id);
    for (entity, mut panel) in &mut panels {
        if panel.initialized && panel.draft_id == draft_id && !localizer.is_changed() {
            if let (Some(draft), Some(view)) = (&editor.0, &mut panel.picker) {
                view.sync(&draft.picker, draft.id, &localizer, &mut commands);
            }
            continue;
        }
        panel.initialized = true;
        panel.draft_id = draft_id;
        panel.picker = None;
        commands
            .entity(entity)
            .despawn_children()
            .with_children(|panel_root| {
                let Some(draft) = editor.0.as_ref() else {
                    panel_root
                        .spawn(Node {
                            padding: UiRect::all(Val::Px(12.0)),
                            ..default()
                        })
                        .with_child((
                            Text::new(localizer.text("browser-palette-editor-empty")),
                            TextColor(theme::TEXT_MUTED),
                            TextFont::from_font_size(13.0),
                        ));
                    return;
                };
                let filename = draft
                    .source
                    .path()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                panel_root
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        flex_wrap: FlexWrap::Wrap,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(Val::Px(10.0)),
                        column_gap: Val::Px(7.0),
                        row_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|header| {
                        header.spawn((
                            Text::new(filename.into_owned()),
                            TextColor(theme::TEXT),
                            TextFont::from_font_size(14.0),
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                margin: UiRect::right(Val::Px(8.0)),
                                ..default()
                            },
                        ));
                        spawn_action_button(
                            header,
                            &localizer.text("browser-palette-discard"),
                            Action::Discard,
                            false,
                        );
                        spawn_action_button(
                            header,
                            &localizer.text("browser-palette-save"),
                            Action::Save,
                            true,
                        );
                    });
                panel_root.spawn((
                    Status,
                    Text::default(),
                    TextColor(theme::TEXT_MUTED),
                    Node {
                        display: Display::None,
                        margin: UiRect::horizontal(Val::Px(10.0)),
                        ..default()
                    },
                ));
                panel_root
                    .spawn(Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        ..default()
                    })
                    .with_children(|row| {
                        crate::feathers::scroll::spawn_vertical_scroll_area(
                            row,
                            ScrollMemoryKey::PaletteEditor,
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(10.0)),
                                row_gap: Val::Px(8.0),
                                ..default()
                            },
                            |body| {
                                panel.picker = Some(draft.picker.spawn(body, &localizer, draft.id));
                            },
                        );
                    });
            });
    }
}

fn change_color(
    event: On<ValueChange<Option<[f32; 4]>>>,
    targets: Query<&PaletteColorTarget>,
    catalog: Res<ProjectEffectCatalog>,
    mut editor: ResMut<PaletteEditor>,
) {
    let (Some(rgba), Ok(target)) = (event.value, targets.get(event.source)) else {
        return;
    };
    let Some(draft) = editor.0.as_mut().filter(|draft| {
        !draft.saving
            && draft.generation == catalog.content_revision().generation
            && draft.picker.color_target(draft.id) == *target
    }) else {
        return;
    };
    draft.picker.colors[draft.picker.selected] =
        [rgba[0], rgba[1], rgba[2]].map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
    draft.error = None;
}

fn activate(
    event: On<Activate>,
    actions: Query<&Action>,
    mut editor: ResMut<PaletteEditor>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    mut commands: Commands,
) {
    let Ok(&action) = actions.get(event.entity) else {
        return;
    };
    let Some(draft) = editor.0.as_mut().filter(|draft| {
        draft.generation == catalog.content_revision().generation
            && draft
                .picker
                .enabled(action, draft.source.palette().colors(), draft.saving)
    }) else {
        return;
    };
    match action {
        Action::Discard => {
            draft.picker.restore(draft.source.palette().colors());
            draft.error = None;
        }
        Action::Save => save(draft, &catalog, &session, &mut commands),
        _ => draft.picker.edit(action),
    }
}

fn save(
    draft: &mut Draft,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    commands: &mut Commands,
) {
    let source = draft.source.clone();
    let palette =
        PaletteAsset::new(draft.picker.colors.clone()).expect("controls enforce palette limits");
    let id = draft.id;
    draft.saving = true;
    draft.error = None;
    io::enqueue_with_rejection(
        commands,
        IoGuard::capture(catalog, session),
        move || {
            let result = source.save_revision(&palette);
            io::completion(move |world| match result {
                Ok(source) => {
                    let path = source.path().to_owned();
                    if let Some(draft) = world
                        .resource_mut::<PaletteEditor>()
                        .0
                        .as_mut()
                        .filter(|draft| draft.id == id)
                    {
                        draft.source = source;
                        draft.saving = false;
                        draft.preview = None;
                    }
                    if let Some(server) = world.get_resource::<AssetServer>() {
                        server.reload(AssetPath::from_path_buf(path));
                    }
                    world.trigger(crate::asset_actions::AssetAction::RefreshProject);
                    io::set_status(world, "browser-palette-saved");
                }
                Err(error) => save_failed(world, id, error.to_string()),
            })
        },
        move |world| {
            let message = world
                .resource::<Localizer>()
                .text("project-operation-queued-cancelled");
            save_failed(world, id, message);
        },
    );
}

fn save_failed(world: &mut World, id: u64, message: String) {
    if let Some(draft) = world
        .resource_mut::<PaletteEditor>()
        .0
        .as_mut()
        .filter(|draft| draft.id == id)
    {
        draft.saving = false;
        draft.error = Some(message);
    }
}

fn sync_controls(
    editor: Res<PaletteEditor>,
    localizer: Res<Localizer>,
    mut chips: Query<(
        Entity,
        &Swatch,
        &mut BackgroundColor,
        &mut Outline,
        &Children,
        Has<Selected>,
    )>,
    mut labels: Query<&mut TextColor>,
    mut status: Query<(&mut Text, &mut Node), With<Status>>,
    actions: Query<(Entity, &Action, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    let Some(draft) = editor.0.as_ref() else {
        return;
    };
    draft
        .picker
        .paint_swatches(&mut chips, &mut labels, &mut commands);
    for (entity, action, disabled) in &actions {
        let enabled = draft
            .picker
            .enabled(*action, draft.source.palette().colors(), draft.saving);
        match (enabled, disabled) {
            (false, false) => {
                commands.entity(entity).insert(InteractionDisabled);
            }
            (true, true) => {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
            _ => {}
        }
    }
    let message = if draft.saving {
        localizer.text("project-operation-running")
    } else if let Some(error) = &draft.error {
        error.clone()
    } else if draft.dirty() {
        localizer.text("browser-palette-unsaved")
    } else {
        String::new()
    };
    let display = if message.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    for (mut text, mut node) in &mut status {
        text.set_if_neq(Text::new(message.clone()));
        if node.display != display {
            node.display = display;
        }
    }
}

fn restore_preview(draft: &mut Draft, images: Option<&mut Assets<Image>>) {
    if let Some(preview) = draft.preview.take()
        && let Some(images) = images
        && images
            .get(preview.handle.id())
            .is_some_and(|image| preview_matches(image, &preview.shown))
    {
        let _ = images.insert(preview.handle.id(), preview.original);
    }
}

fn preview_matches(image: &Image, colors: &[[u8; 3]]) -> bool {
    image.data.as_deref().is_some_and(|pixels| {
        pixels.len() == colors.len() * 4
            && pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(colors)
                .all(|(pixel, swatch)| pixel[..3] == swatch[..] && pixel[3] == 255)
    })
}

fn sync_preview(
    mut editor: ResMut<PaletteEditor>,
    catalog: Res<ProjectEffectCatalog>,
    server: Option<Res<AssetServer>>,
    images: Option<ResMut<Assets<Image>>>,
    layout: Option<Res<WorkspaceLayout>>,
) {
    let mut images = images;
    if editor
        .0
        .as_ref()
        .is_some_and(|draft| draft.generation != catalog.content_revision().generation)
    {
        if let Some(mut draft) = editor.0.take() {
            restore_preview(&mut draft, images.as_deref_mut());
        }
        return;
    }
    let Some(draft) = editor.bypass_change_detection().0.as_mut() else {
        return;
    };
    if !layout.is_some_and(|layout| layout.is_visible(ToolPanel::PaletteEditor)) || !draft.dirty() {
        restore_preview(draft, images.as_deref_mut());
        return;
    }
    let (Some(server), Some(mut images)) = (server, images) else {
        return;
    };
    let preview = if let Some(preview) = &mut draft.preview {
        preview
    } else {
        let Some(handle) =
            server.get_handle::<Image>(AssetPath::from_path_buf(draft.source.path().to_owned()))
        else {
            return;
        };
        let Some(original) = images.get(handle.id()).cloned() else {
            return;
        };
        draft.preview.insert(PreviewTexture {
            handle,
            original,
            shown: Vec::new(),
        })
    };
    // An in-flight asset reload can replace this handle while a draft is open.
    // Keep the new disk image for discard and put the draft back on the same handle.
    let new_disk_image = images
        .get(preview.handle.id())
        .filter(|image| !preview.shown.is_empty() && !preview_matches(image, &preview.shown));
    let replaced = new_disk_image.is_some();
    if let Some(image) = new_disk_image {
        preview.original = image.clone();
    }
    if replaced || preview.shown != draft.picker.colors {
        let data = draft
            .picker
            .colors
            .iter()
            .flat_map(|&[red, green, blue]| [red, green, blue, 255])
            .collect();
        let image = Image::new(
            Extent3d {
                width: draft.picker.colors.len() as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        if images.insert(preview.handle.id(), image).is_ok() {
            preview.shown.clone_from(&draft.picker.colors);
        }
    }
}

#[cfg(test)]
#[path = "tests/palette_editor.rs"]
pub(super) mod tests;
