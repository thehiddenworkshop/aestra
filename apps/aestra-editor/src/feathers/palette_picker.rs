//! Feathers controls for selecting, editing, and ordering opaque palette swatches.

use super::{
    action_activation::FeathersActionButton,
    button::spawn_action_button,
    color_picker::{ColorPickerLabels, spawn_rgb_color_picker},
    scenes,
};
use crate::{Localizer, ScrollMemoryKey, theme};
use aestra_core::MAX_PALETTE_COLORS;
use bevy::{prelude::*, ui::Selected};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteAction {
    Select(usize),
    Add,
    Remove,
    MoveLeft,
    MoveRight,
    Save,
    Discard,
}

#[derive(Component)]
pub(crate) struct Swatch(pub(crate) usize);

#[derive(Component)]
pub(crate) struct PalettePickerRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaletteColorTarget {
    pub(crate) draft: u64,
    pub(crate) selection: u64,
}

pub(crate) struct PalettePicker {
    pub(crate) colors: Vec<[u8; 3]>,
    pub(crate) selected: usize,
    pub(crate) selection_revision: u64,
}

pub(crate) struct PalettePickerView {
    grid: Entity,
    field: Entity,
    count: usize,
    target: PaletteColorTarget,
}

impl PalettePicker {
    pub(crate) fn new(colors: Vec<[u8; 3]>) -> Self {
        Self {
            colors,
            selected: 0,
            selection_revision: 0,
        }
    }

    pub(crate) fn spawn(
        &self,
        panel: &mut ChildSpawnerCommands,
        localizer: &Localizer,
        draft: u64,
    ) -> PalettePickerView {
        let mut grid = Entity::PLACEHOLDER;
        let mut field = Entity::PLACEHOLDER;
        let target = self.color_target(draft);
        panel
            .spawn((
                PalettePickerRoot,
                Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::FlexStart,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(12.0),
                    row_gap: Val::Px(12.0),
                    ..default()
                },
            ))
            .with_children(|body| {
                body.spawn(card(280.0)).with_children(|swatches| {
                    section_label(swatches, localizer.text("browser-palette-swatches"));
                    swatches
                        .spawn(Node {
                            height: Val::Px(158.0),
                            ..default()
                        })
                        .with_children(|row| {
                            crate::feathers::scroll::spawn_vertical_scroll_area(
                                row,
                                ScrollMemoryKey::PaletteEditorSwatches,
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                                |scroll| {
                                    grid = scroll
                                        .spawn(Node {
                                            width: Val::Percent(100.0),
                                            padding: UiRect::all(Val::Px(3.0)),
                                            flex_wrap: FlexWrap::Wrap,
                                            column_gap: Val::Px(6.0),
                                            row_gap: Val::Px(6.0),
                                            ..default()
                                        })
                                        .with_children(|grid| self.spawn_swatches(grid, localizer))
                                        .id();
                                },
                            );
                        });
                    swatches
                        .spawn(Node {
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(5.0),
                            row_gap: Val::Px(5.0),
                            ..default()
                        })
                        .with_children(|row| {
                            for (id, action) in [
                                ("browser-palette-add", PaletteAction::Add),
                                ("browser-palette-remove", PaletteAction::Remove),
                                ("browser-palette-earlier", PaletteAction::MoveLeft),
                                ("browser-palette-later", PaletteAction::MoveRight),
                            ] {
                                spawn_action_button(row, &localizer.text(id), action, false);
                            }
                        });
                });
                field = body
                    .spawn(card(245.0))
                    .with_children(|field| self.spawn_color(field, target, localizer))
                    .id();
            });
        PalettePickerView {
            grid,
            field,
            target,
            count: self.colors.len(),
        }
    }

    pub(crate) fn color_target(&self, draft: u64) -> PaletteColorTarget {
        PaletteColorTarget {
            draft,
            selection: self.selection_revision,
        }
    }

    pub(crate) fn enabled(
        &self,
        action: PaletteAction,
        original: &[[u8; 3]],
        saving: bool,
    ) -> bool {
        !saving
            && match action {
                PaletteAction::Select(index) => index < self.colors.len(),
                PaletteAction::Add => self.colors.len() < MAX_PALETTE_COLORS,
                PaletteAction::Remove => self.colors.len() > 1,
                PaletteAction::MoveLeft => self.selected > 0,
                PaletteAction::MoveRight => self.selected + 1 < self.colors.len(),
                PaletteAction::Save | PaletteAction::Discard => self.colors != original,
            }
    }

    pub(crate) fn edit(&mut self, action: PaletteAction) {
        match action {
            PaletteAction::Select(index) => {
                if self.selected == index {
                    return;
                }
                self.selected = index;
            }
            PaletteAction::Add => {
                self.colors
                    .insert(self.selected + 1, self.colors[self.selected]);
                self.selected += 1;
            }
            PaletteAction::Remove => {
                self.colors.remove(self.selected);
                self.selected = self.selected.min(self.colors.len() - 1);
            }
            PaletteAction::MoveLeft | PaletteAction::MoveRight => {
                let next = if action == PaletteAction::MoveLeft {
                    self.selected - 1
                } else {
                    self.selected + 1
                };
                self.colors.swap(self.selected, next);
                self.selected = next;
            }
            PaletteAction::Save | PaletteAction::Discard => return,
        }
        self.selection_revision = self.selection_revision.wrapping_add(1);
    }

    pub(crate) fn restore(&mut self, colors: &[[u8; 3]]) {
        self.colors = colors.to_vec();
        self.selected = self.selected.min(self.colors.len() - 1);
        self.selection_revision = self.selection_revision.wrapping_add(1);
    }

    pub(crate) fn paint_swatches(
        &self,
        chips: &mut Query<(
            Entity,
            &Swatch,
            &mut BackgroundColor,
            &mut Outline,
            &Children,
            Has<Selected>,
        )>,
        labels: &mut Query<&mut TextColor>,
        commands: &mut Commands,
    ) {
        for (entity, chip, mut background, mut outline, children, was_selected) in chips {
            let Some(&[r, g, b]) = self.colors.get(chip.0) else {
                continue;
            };
            let selected = chip.0 == self.selected;
            background.set_if_neq(BackgroundColor(Color::srgb_u8(r, g, b)));
            outline.width = Val::Px(if selected { 2.0 } else { 0.0 });
            outline.color = theme::TEXT;
            if selected != was_selected {
                if selected {
                    commands.entity(entity).insert(Selected);
                } else {
                    commands.entity(entity).remove::<Selected>();
                }
            }
            if let Some(child) = children.first()
                && let Ok(mut text) = labels.get_mut(*child)
            {
                text.set_if_neq(TextColor(swatch_foreground([r, g, b])));
            }
        }
    }

    fn spawn_swatches(&self, parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
        for (index, &[r, g, b]) in self.colors.iter().enumerate() {
            let label = swatch_label(index, localizer);
            let selected = index == self.selected;
            let mut button = parent.spawn_empty();
            button
                .apply_scene(scenes::feathers_button())
                .insert((
                    PaletteAction::Select(index),
                    FeathersActionButton,
                    AccessibleLabel(label),
                    Node {
                        width: Val::Px(36.0),
                        height: Val::Px(34.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .with_children(|button| {
                    let mut chip = button.spawn((
                        Swatch(index),
                        BackgroundColor(Color::srgb_u8(r, g, b)),
                        Outline::new(
                            Val::Px(if selected { 2.0 } else { 0.0 }),
                            Val::Px(1.0),
                            theme::TEXT,
                        ),
                        Pickable::IGNORE,
                        Node {
                            width: Val::Px(30.0),
                            height: Val::Px(28.0),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                    ));
                    if selected {
                        chip.insert(Selected);
                    }
                    chip.with_child((
                        Text::new((index + 1).to_string()),
                        TextFont::from_font_size(10.0),
                        TextColor(swatch_foreground([r, g, b])),
                        Pickable::IGNORE,
                    ));
                });
        }
    }

    fn spawn_color(
        &self,
        parent: &mut ChildSpawnerCommands,
        target: PaletteColorTarget,
        localizer: &Localizer,
    ) {
        let accessible = swatch_label(self.selected, localizer);
        section_label(parent, accessible.clone());
        spawn_rgb_color_picker(
            parent,
            self.colors[self.selected],
            ColorPickerLabels {
                accessible,
                hue_saturation: localizer.text("timeline-color-picker-hue-saturation"),
                lightness: localizer.text("timeline-color-picker-lightness"),
                alpha: String::new(),
                automatic: String::new(),
                rgb: localizer.text("timeline-color-picker-rgb"),
                hsl: localizer.text("timeline-color-picker-hsl"),
                red: localizer.text("timeline-color-picker-red"),
                green: localizer.text("timeline-color-picker-green"),
                blue: localizer.text("timeline-color-picker-blue"),
                hue: localizer.text("timeline-color-picker-hue"),
                saturation: localizer.text("timeline-color-picker-saturation"),
                hex: localizer.text("timeline-color-picker-hex"),
            },
            target,
        );
    }
}

impl PalettePickerView {
    pub(crate) fn sync(
        &mut self,
        picker: &PalettePicker,
        draft: u64,
        localizer: &Localizer,
        commands: &mut Commands,
    ) {
        if self.count != picker.colors.len() {
            self.count = picker.colors.len();
            commands
                .entity(self.grid)
                .despawn_children()
                .with_children(|grid| picker.spawn_swatches(grid, localizer));
        }
        let target = picker.color_target(draft);
        if self.target != target {
            self.target = target;
            commands
                .entity(self.field)
                .despawn_children()
                .with_children(|field| picker.spawn_color(field, target, localizer));
        }
    }
}

fn card(width: f32) -> impl Bundle {
    (
        Node {
            min_width: Val::Px(0.0),
            max_width: Val::Px(420.0),
            flex_basis: Val::Px(width),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(10.0),
            padding: UiRect::all(Val::Px(12.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL_LIGHT),
        BorderColor::all(theme::BORDER),
    )
}

fn section_label(parent: &mut ChildSpawnerCommands, label: String) {
    parent.spawn((
        Text::new(label),
        TextColor(theme::TEXT),
        TextFont::from_font_size(13.0),
    ));
}

fn swatch_label(index: usize, localizer: &Localizer) -> String {
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("number", (index + 1) as i64);
    localizer.text_with("browser-palette-swatch", &args)
}

fn swatch_foreground([r, g, b]: [u8; 3]) -> Color {
    if u32::from(r) * 3 + u32::from(g) * 6 + u32::from(b) > 1400 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}
