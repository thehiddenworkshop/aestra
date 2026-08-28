use crate::{
    ComboOption, EditorAction, EditorNativeControl, Localizer, layer_color, mini_button,
    session::EditorSession, spawn_combo_control, theme,
};
use aestra_bevy::EmitterId;
use bevy::{
    feathers::cursor::{EntityCursor, OverrideCursor},
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    picking::{
        events::{Click, Drag, DragEnd, DragStart, Pointer, Press},
        pointer::PointerButton,
    },
    prelude::*,
    ui::RelativeCursorPosition,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TimelineSet {
    Input,
    Visuals,
}

pub(crate) struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        let duration = app
            .world()
            .get_resource::<EditorSession>()
            .map_or(1.0, EditorSession::playback_duration);
        app.insert_resource(TimelineState::framed(duration))
            .add_systems(Update, navigate_timeline.in_set(TimelineSet::Input))
            .add_systems(
                Update,
                (
                    update_timeline_time_label,
                    update_timeline_visuals,
                    update_timeline_scrollbar,
                )
                    .chain()
                    .in_set(TimelineSet::Visuals),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EFFECT_PATH, EFFECT_SOURCE};

    #[test]
    fn timeline_cursor_coordinates_cover_the_full_canvas() {
        assert_eq!(timeline_cursor_fraction(-0.5), 0.0);
        assert_eq!(timeline_cursor_fraction(0.0), 0.5);
        assert_eq!(timeline_cursor_fraction(0.5), 1.0);
    }

    #[test]
    fn timeline_zoom_keeps_the_time_under_the_pointer() {
        let mut state = TimelineState::default();
        state.frame_all(10.0);
        let anchor = state.view.time_at(0.73);

        state.zoom_at(anchor, 0.5, 10.0, 60);

        assert!((state.view.time_at(0.73) - anchor).abs() < 0.000_1);
        assert!((state.view.span() - 5.0).abs() < 0.000_1);
    }

    #[test]
    fn timeline_pan_stays_inside_the_effect() {
        let mut state = TimelineState {
            view: TimelineView {
                start: 2.0,
                end: 4.0,
            },
            ..default()
        };

        state.pan_by(-10.0, 8.0);
        assert_eq!(state.view.start, 0.0);
        assert_eq!(state.view.end, 2.0);

        state.pan_by(20.0, 8.0);
        assert_eq!(state.view.start, 6.0);
        assert_eq!(state.view.end, 8.0);
    }

    #[test]
    fn timeline_ruler_uses_human_readable_intervals() {
        assert_eq!(nice_timeline_step(10.0, 1_000.0), 1.0);
        assert_eq!(nice_timeline_step(2.8, 1_000.0), 0.5);
        assert_eq!(nice_timeline_step(0.2, 1_000.0), 0.02);
    }

    #[test]
    fn timeline_timing_commit_is_one_undoable_command() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let emitter = session.effect.emitters[0].clone();

        assert!(session.set_emitter_timing(
            emitter.id,
            emitter.start_time + 0.1,
            emitter.duration - 0.1,
            "Moved emitter on timeline",
        ));
        assert!(session.can_undo());
        session.undo();

        let restored = session
            .effect
            .emitters
            .iter()
            .find(|candidate| candidate.id == emitter.id)
            .unwrap();
        assert_eq!(restored.start_time, emitter.start_time);
        assert_eq!(restored.duration, emitter.duration);
    }

    #[test]
    fn timeline_visual_queries_initialize_without_aliasing() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let timeline = TimelineState::framed(session.playback_duration());
        let mut app = App::new();
        app.insert_resource(session);
        app.insert_resource(timeline);
        app.add_systems(
            Update,
            (update_timeline_visuals, update_timeline_scrollbar).chain(),
        );

        app.update();
    }
}

#[allow(clippy::type_complexity)]
fn update_timeline_visuals(
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    mut clips: Query<(&TimelineClip, &mut Node), Without<Playhead>>,
    mut playheads: Query<&mut Node, (With<Playhead>, Without<TimelineClip>)>,
    mut guides: Query<
        &mut Node,
        (
            With<TimelineSnapGuide>,
            Without<TimelineClip>,
            Without<Playhead>,
            Without<TimelineRulerTick>,
        ),
    >,
    mut ticks: Query<
        (&TimelineRulerTick, &Children, &mut Node),
        (
            Without<TimelineClip>,
            Without<Playhead>,
            Without<TimelineSnapGuide>,
        ),
    >,
    mut texts: Query<&mut Text>,
) {
    state.ensure_duration(session.playback_duration());
    let view = state.view;
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(320.0);

    for (clip, mut node) in &mut clips {
        let Some(emitter) = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == clip.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        let (start, duration) = state
            .drag
            .filter(|drag| drag.emitter == clip.emitter)
            .map_or((emitter.start_time, emitter.duration), |drag| {
                (drag.current_start, drag.current_duration)
            });
        let end = start + duration;
        let visible_start = start.max(view.start);
        let visible_end = end.min(view.end);
        if visible_end <= visible_start {
            node.display = Display::None;
            continue;
        }
        node.display = Display::Flex;
        node.left = Val::Percent(view.normalized_time(visible_start) * 100.0);
        node.width =
            Val::Percent(((visible_end - visible_start) / view.span() * 100.0).clamp(0.05, 100.0));
    }

    let playhead_position = view.normalized_time(session.time());
    for mut node in &mut playheads {
        node.display = if (0.0..=1.0).contains(&playhead_position) {
            Display::Flex
        } else {
            Display::None
        };
        node.left = Val::Percent(playhead_position.clamp(0.0, 1.0) * 100.0);
    }
    for mut node in &mut guides {
        if let Some(time) = state.snap_guide
            && (view.start..=view.end).contains(&time)
        {
            node.display = Display::Flex;
            node.left = Val::Percent(view.normalized_time(time) * 100.0);
        } else {
            node.display = Display::None;
        }
    }

    let step = nice_timeline_step(view.span(), width);
    let first = (view.start / step).ceil() * step;
    for (tick, children, mut node) in &mut ticks {
        let time = first + tick.0 as f32 * step;
        if time > view.end + step * 0.001 {
            node.display = Display::None;
            continue;
        }
        node.display = Display::Flex;
        node.left = Val::Percent(view.normalized_time(time) * 100.0);
        if let Some(child) = children.first()
            && let Ok(mut text) = texts.get_mut(*child)
        {
            text.0 = format_timeline_tick(time, step);
        }
    }
}

fn update_timeline_scrollbar(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut tracks: Query<
        &mut Node,
        (
            With<TimelineScrollbarTrack>,
            Without<TimelineScrollbarThumb>,
        ),
    >,
    mut thumbs: Query<
        &mut Node,
        (
            With<TimelineScrollbarThumb>,
            Without<TimelineScrollbarTrack>,
        ),
    >,
) {
    let duration = session.playback_duration().max(0.05);
    let visible_ratio = (state.view.span() / duration).clamp(0.0, 1.0);
    let overflow = visible_ratio < 0.999;
    for mut node in &mut tracks {
        node.display = if overflow {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut thumbs {
        node.left = Val::Percent((state.view.start / duration * 100.0).clamp(0.0, 100.0));
        node.width = Val::Percent((visible_ratio * 100.0).clamp(2.0, 100.0));
    }
}

fn update_timeline_time_label(
    session: Res<EditorSession>,
    mut labels: Query<&mut Text, With<TimeLabel>>,
) {
    if !session.is_changed() {
        return;
    }
    for mut text in &mut labels {
        text.0 = format!(
            "F{:05}  ·  {:02}:{:06.3}  /  00:{:06.3}  ·  {}",
            session.frame(),
            0,
            session.time(),
            session.playback_duration(),
            session.seek_status()
        );
    }
}

fn nice_timeline_step(span: f32, width: f32) -> f32 {
    let target_ticks = (width / 96.0).clamp(2.0, 24.0);
    let raw = (span / target_ticks).max(0.000_001);
    let magnitude = 10.0_f32.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let factor = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    factor * magnitude
}

fn format_timeline_tick(time: f32, step: f32) -> String {
    if step >= 1.0 {
        format!("{time:.1}")
    } else if step >= 0.1 {
        format!("{time:.2}")
    } else {
        format!("{time:.3}")
    }
}

fn snap_timeline_boundary(
    candidate: f32,
    emitter: EmitterId,
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    match mode {
        TimelineSnapMode::None => (candidate, None),
        TimelineSnapMode::Frames => {
            let frame = 1.0 / session.clock.tick_rate().max(1) as f32;
            let snapped = (candidate / frame).round() * frame;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Seconds => {
            let interval = nice_timeline_step(view.span(), canvas_width) / 5.0;
            let snapped = (candidate / interval).round() * interval;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Smart => {
            let threshold = view.span() / canvas_width.max(1.0) * 9.0;
            let frame = 1.0 / session.clock.tick_rate().max(1) as f32;
            let mut targets = vec![
                0.0,
                session.playback_duration(),
                session.time(),
                (candidate / frame).round() * frame,
            ];
            for other in &session.effect.emitters {
                if other.id != emitter {
                    targets.push(other.start_time);
                    targets.push(other.start_time + other.duration);
                }
            }
            let nearest = targets.into_iter().min_by(|left, right| {
                (candidate - *left)
                    .abs()
                    .total_cmp(&(candidate - *right).abs())
            });
            nearest
                .filter(|target| (candidate - *target).abs() <= threshold)
                .map_or((candidate, None), |target| (target, Some(target)))
        }
    }
}

fn snap_moved_timing(
    start: f32,
    duration: f32,
    emitter: EmitterId,
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    let start_snap = snap_timeline_boundary(start, emitter, session, mode, view, canvas_width);
    if mode != TimelineSnapMode::Smart {
        return start_snap;
    }
    let end = start + duration;
    let end_snap = snap_timeline_boundary(end, emitter, session, mode, view, canvas_width);
    let start_delta = (start_snap.0 - start).abs();
    let end_delta = (end_snap.0 - end).abs();
    match (start_snap.1, end_snap.1) {
        (None, Some(guide)) => (start + end_snap.0 - end, Some(guide)),
        (Some(_), Some(guide)) if end_delta < start_delta => {
            (start + end_snap.0 - end, Some(guide))
        }
        _ => start_snap,
    }
}

#[derive(Component)]
struct TimelineCanvas;

#[derive(Component, Clone, Copy)]
struct TimelineClip {
    emitter: EmitterId,
}

#[derive(Component, Clone, Copy)]
struct TimelineClipInteraction {
    emitter: EmitterId,
    kind: TimelineDragKind,
}

#[derive(Component)]
struct TimelineRulerTick(usize);

#[derive(Component)]
struct TimelineSnapGuide;

#[derive(Component)]
struct TimelineScrollbarTrack;

#[derive(Component)]
struct TimelineScrollbarThumb;

#[derive(Component)]
struct Playhead;

#[derive(Component)]
struct TimeLabel;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TimelineSnapMode {
    None,
    Frames,
    Seconds,
    #[default]
    Smart,
}

impl TimelineSnapMode {
    const ALL: [Self; 4] = [Self::None, Self::Frames, Self::Seconds, Self::Smart];

    fn message_id(self) -> &'static str {
        match self {
            Self::None => "timeline-snap-off",
            Self::Frames => "timeline-snap-frames",
            Self::Seconds => "timeline-snap-time",
            Self::Smart => "timeline-snap-smart",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimelineDragKind {
    Move,
    TrimStart,
    TrimEnd,
}

#[derive(Clone, Copy, Debug)]
struct TimelineDrag {
    emitter: EmitterId,
    kind: TimelineDragKind,
    pointer_start: f32,
    original_start: f32,
    original_duration: f32,
    current_start: f32,
    current_duration: f32,
}

#[derive(Clone, Copy, Debug)]
struct TimelineScrollbarDrag {
    view_start: f32,
}

#[derive(Clone, Copy, Debug)]
struct TimelineView {
    start: f32,
    end: f32,
}

impl TimelineView {
    fn span(self) -> f32 {
        (self.end - self.start).max(0.000_1)
    }

    fn time_at(self, normalized: f32) -> f32 {
        self.start + normalized.clamp(0.0, 1.0) * self.span()
    }

    fn normalized_time(self, time: f32) -> f32 {
        (time - self.start) / self.span()
    }
}

#[derive(Resource, Debug)]
pub(crate) struct TimelineState {
    view: TimelineView,
    snap: TimelineSnapMode,
    drag: Option<TimelineDrag>,
    snap_guide: Option<f32>,
    panning: bool,
    scrollbar_drag: Option<TimelineScrollbarDrag>,
    known_duration: f32,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self::framed(1.0)
    }
}

impl TimelineState {
    fn framed(duration: f32) -> Self {
        let duration = duration.max(0.05);
        Self {
            view: TimelineView {
                start: 0.0,
                end: duration,
            },
            snap: TimelineSnapMode::Smart,
            drag: None,
            snap_guide: None,
            panning: false,
            scrollbar_drag: None,
            known_duration: duration,
        }
    }

    pub(crate) fn set_snap(&mut self, snap: TimelineSnapMode) -> bool {
        if self.snap == snap {
            return false;
        }
        self.snap = snap;
        self.snap_guide = None;
        true
    }

    pub(crate) fn frame_all(&mut self, duration: f32) {
        let duration = duration.max(0.05);
        self.view = TimelineView {
            start: 0.0,
            end: duration,
        };
        self.known_duration = duration;
        self.snap_guide = None;
    }

    fn ensure_duration(&mut self, duration: f32) {
        let duration = duration.max(0.05);
        if (duration - self.known_duration).abs() <= f32::EPSILON {
            return;
        }
        let was_framed =
            self.view.start <= f32::EPSILON && (self.view.end - self.known_duration).abs() < 0.001;
        self.known_duration = duration;
        if was_framed {
            self.frame_all(duration);
        } else {
            self.clamp_view(duration);
        }
    }

    fn zoom_at(&mut self, anchor: f32, factor: f32, duration: f32, tick_rate: u32) {
        let duration = duration.max(0.05);
        let minimum_span = (4.0 / tick_rate.max(1) as f32).max(0.01);
        let old_span = self.view.span();
        let new_span = (old_span * factor).clamp(minimum_span.min(duration), duration);
        let anchor_ratio = ((anchor - self.view.start) / old_span).clamp(0.0, 1.0);
        self.view.start = anchor - new_span * anchor_ratio;
        self.view.end = self.view.start + new_span;
        self.clamp_view(duration);
    }

    fn pan_by(&mut self, delta: f32, duration: f32) {
        self.view.start += delta;
        self.view.end += delta;
        self.clamp_view(duration.max(0.05));
    }

    fn clamp_view(&mut self, duration: f32) {
        let span = self.view.span().min(duration);
        self.view.start = self.view.start.clamp(0.0, (duration - span).max(0.0));
        self.view.end = self.view.start + span;
    }
}

pub(crate) fn spawn_timeline(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|timeline| {
            timeline
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(14.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new("00:00.000  /  00:02.800"),
                        TimeLabel,
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                    mini_button(header, "<", EditorAction::StepFrame(-1));
                    mini_button(header, ">", EditorAction::StepFrame(1));
                    mini_button(
                        header,
                        &localizer.text("timeline-frame-all"),
                        EditorAction::FrameTimeline,
                    );
                    let snap_options = TimelineSnapMode::ALL
                        .into_iter()
                        .map(|mode| ComboOption {
                            label: localizer.text(mode.message_id()),
                            selected: state.snap == mode,
                            action: EditorAction::SetTimelineSnap(mode),
                        })
                        .collect::<Vec<_>>();
                    spawn_combo_control(
                        header,
                        &localizer.text(state.snap.message_id()),
                        &localizer.text("timeline-snapping-description"),
                        &snap_options,
                        112.0,
                    );
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new(format!(
                            "{} {}  ·  {} {:016x}",
                            session.clock.tick_rate(),
                            localizer.text("timeline-hertz"),
                            localizer.text("timeline-seed"),
                            session.preview_seed
                        )),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                    mini_button(header, "-", EditorAction::AdjustPreviewSeed(-1));
                    mini_button(header, "+", EditorAction::AdjustPreviewSeed(1));
                    header.spawn((
                        Text::new(format!(
                            "{} {:.2}s",
                            localizer.text("timeline-duration"),
                            session.playback_duration()
                        )),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    mini_button(header, "-", EditorAction::EffectDuration(-0.25));
                    mini_button(header, "+", EditorAction::EffectDuration(0.25));
                });
            timeline
                .spawn(Node {
                    flex_grow: 1.0,
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn((
                        Node {
                            width: Val::Px(224.0),
                            height: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::top(Val::Px(25.0)),
                            border: UiRect::right(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL_DARK),
                        BorderColor::all(theme::BORDER),
                    ))
                    .with_children(|labels| {
                        for (index, emitter) in session.effect.emitters.iter().enumerate() {
                            labels.spawn((
                                Text::new(format!("  {:02}   {}", index + 1, emitter.name)),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT_MUTED),
                                Node {
                                    height: Val::Px(31.0),
                                    padding: UiRect::top(Val::Px(8.0)),
                                    ..default()
                                },
                            ));
                        }
                    });
                    body.spawn((
                        Button,
                        EditorNativeControl,
                        TimelineCanvas,
                        RelativeCursorPosition::default(),
                        Node {
                            flex_grow: 1.0,
                            height: Val::Percent(100.0),
                            position_type: PositionType::Relative,
                            padding: UiRect::top(Val::Px(25.0)),
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme::TIMELINE_BG),
                    ))
                    .observe(seek_timeline_on_press)
                    .observe(seek_timeline_on_drag)
                    .with_children(|tracks| {
                        spawn_ruler(tracks);
                        for (index, emitter) in session.effect.emitters.iter().enumerate() {
                            tracks
                                .spawn(Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(31.0),
                                    position_type: PositionType::Relative,
                                    border: UiRect::bottom(Val::Px(1.0)),
                                    ..default()
                                })
                                .with_children(|track| {
                                    track
                                        .spawn((
                                            TimelineClip {
                                                emitter: emitter.id,
                                            },
                                            Node {
                                                position_type: PositionType::Absolute,
                                                left: Val::Percent(0.0),
                                                top: Val::Px(5.0),
                                                width: Val::Percent(1.0),
                                                height: Val::Px(21.0),
                                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                                border: UiRect::all(Val::Px(1.0)),
                                                overflow: Overflow::clip(),
                                                ..default()
                                            },
                                            BackgroundColor(layer_color(index).with_alpha(0.28)),
                                            BorderColor::all(layer_color(index)),
                                        ))
                                        .with_children(|clip| {
                                            clip.spawn((
                                                Button,
                                                EditorNativeControl,
                                                TimelineClipInteraction {
                                                    emitter: emitter.id,
                                                    kind: TimelineDragKind::Move,
                                                },
                                                EntityCursor::System(SystemCursorIcon::Grab),
                                                Node {
                                                    position_type: PositionType::Absolute,
                                                    left: Val::Px(8.0),
                                                    right: Val::Px(8.0),
                                                    top: Val::Px(0.0),
                                                    bottom: Val::Px(0.0),
                                                    ..default()
                                                },
                                                BackgroundColor(Color::NONE),
                                            ))
                                            .observe(begin_timeline_clip_drag)
                                            .observe(move_timeline_clip_drag)
                                            .observe(finish_timeline_clip_drag)
                                            .observe(select_timeline_clip)
                                            .observe(stop_timeline_control_press);
                                            for (kind, left, right) in [
                                                (
                                                    TimelineDragKind::TrimStart,
                                                    Val::Px(0.0),
                                                    Val::Auto,
                                                ),
                                                (
                                                    TimelineDragKind::TrimEnd,
                                                    Val::Auto,
                                                    Val::Px(0.0),
                                                ),
                                            ] {
                                                clip.spawn((
                                                    Button,
                                                    EditorNativeControl,
                                                    TimelineClipInteraction {
                                                        emitter: emitter.id,
                                                        kind,
                                                    },
                                                    EntityCursor::System(
                                                        SystemCursorIcon::EwResize,
                                                    ),
                                                    Node {
                                                        position_type: PositionType::Absolute,
                                                        left,
                                                        right,
                                                        top: Val::Px(0.0),
                                                        width: Val::Px(8.0),
                                                        height: Val::Percent(100.0),
                                                        align_items: AlignItems::Center,
                                                        justify_content: JustifyContent::Center,
                                                        ..default()
                                                    },
                                                    BackgroundColor(Color::NONE),
                                                ))
                                                .observe(begin_timeline_clip_drag)
                                                .observe(move_timeline_clip_drag)
                                                .observe(finish_timeline_clip_drag)
                                                .observe(select_timeline_clip)
                                                .observe(stop_timeline_control_press)
                                                .with_child((
                                                    Node {
                                                        width: Val::Px(2.0),
                                                        height: Val::Px(13.0),
                                                        ..default()
                                                    },
                                                    BackgroundColor(layer_color(index)),
                                                    Pickable::IGNORE,
                                                ));
                                            }
                                        });
                                });
                        }
                        tracks.spawn((
                            TimelineSnapGuide,
                            Node {
                                display: Display::None,
                                position_type: PositionType::Absolute,
                                left: Val::Percent(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(1.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(theme::ACCENT),
                            Pickable::IGNORE,
                            ZIndex(3),
                        ));
                        tracks.spawn((
                            Playhead,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(1.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(theme::PLAYHEAD),
                            Pickable::IGNORE,
                            ZIndex(2),
                        ));
                        tracks
                            .spawn((
                                TimelineScrollbarTrack,
                                RelativeCursorPosition::default(),
                                Node {
                                    display: Display::None,
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(6.0),
                                    right: Val::Px(6.0),
                                    bottom: Val::Px(3.0),
                                    height: Val::Px(10.0),
                                    border_radius: BorderRadius::all(Val::Px(5.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::PANEL_LIGHT.with_alpha(0.88)),
                                ZIndex(5),
                            ))
                            .with_child((
                                Button,
                                EditorNativeControl,
                                TimelineScrollbarThumb,
                                EntityCursor::System(SystemCursorIcon::Grab),
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Percent(0.0),
                                    top: Val::Px(2.0),
                                    width: Val::Percent(100.0),
                                    min_width: Val::Px(20.0),
                                    height: Val::Px(6.0),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::TEXT_FAINT.with_alpha(0.75)),
                            ))
                            .observe(begin_timeline_scrollbar_drag)
                            .observe(move_timeline_scrollbar_drag)
                            .observe(finish_timeline_scrollbar_drag)
                            .observe(stop_timeline_control_press);
                    });
                });
        });
}

fn spawn_ruler(parent: &mut ChildSpawnerCommands) {
    for index in 0..32 {
        parent
            .spawn((
                TimelineRulerTick(index),
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    left: Val::Percent(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(1.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(theme::BORDER.with_alpha(0.55)),
                Pickable::IGNORE,
            ))
            .with_child((
                Text::new("0.0"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(4.0),
                    top: Val::Px(4.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
    }
}

fn navigate_timeline(
    mut wheel: MessageReader<MouseWheel>,
    mut motion: MessageReader<MouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    canvases: Query<(&RelativeCursorPosition, &ComputedNode), With<TimelineCanvas>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape)
        && (state.drag.take().is_some() || state.scrollbar_drag.take().is_some())
    {
        state.snap_guide = None;
        override_cursor.0 = None;
        **cursor = CursorIcon::System(SystemCursorIcon::Default);
    }
    let pointer_delta = motion
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let hovered = canvases.iter().find(|(cursor, _)| cursor.cursor_over());

    if buttons.just_pressed(MouseButton::Middle) && hovered.is_some() {
        state.panning = true;
    }
    if buttons.just_released(MouseButton::Middle) {
        state.panning = false;
    }
    if state.panning
        && buttons.pressed(MouseButton::Middle)
        && let Some((_, canvas)) = hovered
    {
        let width = canvas.size().x.max(1.0);
        let delta_time = -pointer_delta.x / width * state.view.span();
        state.pan_by(delta_time, session.playback_duration());
    }

    let scroll = wheel.read().fold(Vec2::ZERO, |sum, event| {
        let scale = match event.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.01,
        };
        sum + Vec2::new(event.x, event.y) * scale
    });
    let Some((cursor, _)) = hovered else {
        return;
    };
    if scroll == Vec2::ZERO {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if shift {
        let amount = if scroll.x.abs() > scroll.y.abs() {
            scroll.x
        } else {
            scroll.y
        };
        let span = state.view.span();
        state.pan_by(-amount * span * 0.08, session.playback_duration());
    } else if let Some(position) = cursor.normalized {
        let anchor = state.view.time_at(timeline_cursor_fraction(position.x));
        state.zoom_at(
            anchor,
            0.82_f32.powf(scroll.y),
            session.playback_duration(),
            session.clock.tick_rate(),
        );
    }
}

fn seek_timeline_on_press(
    press: On<Pointer<Press>>,
    timelines: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    state: Res<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if press.button == PointerButton::Primary {
        seek_timeline_to_pointer(press.event_target(), &timelines, &state, &mut session);
    }
}

fn seek_timeline_on_drag(
    drag: On<Pointer<Drag>>,
    timelines: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    state: Res<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if drag.button == PointerButton::Primary {
        seek_timeline_to_pointer(drag.event_target(), &timelines, &state, &mut session);
    }
}

fn seek_timeline_to_pointer(
    entity: Entity,
    timelines: &Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    state: &TimelineState,
    session: &mut EditorSession,
) {
    let Ok(cursor) = timelines.get(entity) else {
        return;
    };
    let Some(position) = cursor.normalized else {
        return;
    };
    session.seek_time(state.view.time_at(timeline_cursor_fraction(position.x)));
}

fn timeline_cursor_fraction(relative_x: f32) -> f32 {
    (relative_x + 0.5).clamp(0.0, 1.0)
}

fn stop_timeline_control_press(mut press: On<Pointer<Press>>) {
    press.propagate(false);
}

fn begin_timeline_clip_drag(
    drag: On<Pointer<DragStart>>,
    targets: Query<&TimelineClipInteraction>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag.event_target()) else {
        return;
    };
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == target.emitter)
    else {
        return;
    };
    state.drag = Some(TimelineDrag {
        emitter: target.emitter,
        kind: target.kind,
        pointer_start: 0.0,
        original_start: emitter.start_time,
        original_duration: emitter.duration,
        current_start: emitter.start_time,
        current_duration: emitter.duration,
    });
    override_cursor.0 = Some(EntityCursor::System(timeline_system_cursor(
        target.kind,
        true,
    )));
    **cursor = timeline_drag_cursor(target.kind, true);
}

fn move_timeline_clip_drag(
    mut drag_event: On<Pointer<Drag>>,
    targets: Query<&TimelineClipInteraction>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    drag_event.propagate(false);
    let Some(mut drag) = state.drag else {
        return;
    };
    if drag.emitter != target.emitter || drag.kind != target.kind {
        return;
    }
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let pointer_time = drag_event.distance.x / width * state.view.span();
    let mut snap_guide = state.snap_guide;
    update_timeline_drag(
        &mut drag,
        pointer_time,
        &session,
        state.snap,
        state.view,
        width,
        &mut snap_guide,
    );
    state.drag = Some(drag);
    state.snap_guide = snap_guide;
}

fn finish_timeline_clip_drag(
    drag_event: On<Pointer<DragEnd>>,
    targets: Query<&TimelineClipInteraction>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.drag.take() else {
        return;
    };
    if drag.emitter != target.emitter || drag.kind != target.kind {
        return;
    }
    state.snap_guide = None;
    override_cursor.0 = None;
    **cursor = timeline_drag_cursor(target.kind, false);
    commit_timeline_drag(&mut session, drag);
}

fn commit_timeline_drag(session: &mut EditorSession, drag: TimelineDrag) {
    let selected_index = session
        .effect
        .emitters
        .iter()
        .position(|emitter| emitter.id == drag.emitter);
    let changed = (drag.current_start - drag.original_start).abs() > 0.000_1
        || (drag.current_duration - drag.original_duration).abs() > 0.000_1;
    if changed {
        let label = match drag.kind {
            TimelineDragKind::Move => "Moved emitter on timeline",
            TimelineDragKind::TrimStart | TimelineDragKind::TrimEnd => {
                "Trimmed emitter on timeline"
            }
        };
        session.set_emitter_timing(
            drag.emitter,
            drag.current_start,
            drag.current_duration,
            label,
        );
    }
    if let Some(index) = selected_index
        && index != session.selected_layer_index()
    {
        session.select_layer(index);
    }
}

fn select_timeline_clip(
    click: On<Pointer<Click>>,
    targets: Query<&TimelineClipInteraction>,
    mut session: ResMut<EditorSession>,
) {
    let Ok(target) = targets.get(click.event_target()) else {
        return;
    };
    if let Some(index) = session
        .effect
        .emitters
        .iter()
        .position(|emitter| emitter.id == target.emitter)
        && index != session.selected_layer_index()
    {
        session.select_layer(index);
    }
}

fn timeline_drag_cursor(kind: TimelineDragKind, active: bool) -> CursorIcon {
    CursorIcon::System(timeline_system_cursor(kind, active))
}

fn timeline_system_cursor(kind: TimelineDragKind, active: bool) -> SystemCursorIcon {
    match kind {
        TimelineDragKind::Move if active => SystemCursorIcon::Grabbing,
        TimelineDragKind::Move => SystemCursorIcon::Grab,
        TimelineDragKind::TrimStart | TimelineDragKind::TrimEnd => SystemCursorIcon::EwResize,
    }
}

fn begin_timeline_scrollbar_drag(
    drag: On<Pointer<DragStart>>,
    thumbs: Query<(), With<TimelineScrollbarThumb>>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if !thumbs.contains(drag.event_target()) {
        return;
    }
    state.scrollbar_drag = Some(TimelineScrollbarDrag {
        view_start: state.view.start,
    });
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
    **cursor = CursorIcon::System(SystemCursorIcon::Grabbing);
}

fn move_timeline_scrollbar_drag(
    mut drag: On<Pointer<Drag>>,
    thumbs: Query<(), With<TimelineScrollbarThumb>>,
    tracks: Query<&ComputedNode, With<TimelineScrollbarTrack>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    if !thumbs.contains(drag.event_target()) {
        return;
    }
    drag.propagate(false);
    let Some(active) = state.scrollbar_drag else {
        return;
    };
    let width = tracks
        .iter()
        .map(|track| track.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let delta = drag.distance.x / width * session.playback_duration();
    let span = state.view.span();
    state.view.start = active.view_start + delta;
    state.view.end = state.view.start + span;
    state.clamp_view(session.playback_duration());
}

fn finish_timeline_scrollbar_drag(
    drag: On<Pointer<DragEnd>>,
    thumbs: Query<(), With<TimelineScrollbarThumb>>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if thumbs.contains(drag.event_target()) {
        state.scrollbar_drag = None;
        override_cursor.0 = None;
        **cursor = CursorIcon::System(SystemCursorIcon::Grab);
    }
}

fn update_timeline_drag(
    drag: &mut TimelineDrag,
    pointer_time: f32,
    session: &EditorSession,
    snap: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
    snap_guide: &mut Option<f32>,
) {
    let effect_duration = session.playback_duration();
    let minimum_duration = (1.0 / session.clock.tick_rate().max(1) as f32).max(0.001);
    let pointer_delta = pointer_time - drag.pointer_start;
    *snap_guide = None;
    match drag.kind {
        TimelineDragKind::Move => {
            let unsnapped = (drag.original_start + pointer_delta)
                .clamp(0.0, (effect_duration - drag.original_duration).max(0.0));
            let (start, guide) = snap_moved_timing(
                unsnapped,
                drag.original_duration,
                drag.emitter,
                session,
                snap,
                view,
                canvas_width,
            );
            drag.current_start =
                start.clamp(0.0, (effect_duration - drag.original_duration).max(0.0));
            drag.current_duration = drag.original_duration;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimStart => {
            let end = drag.original_start + drag.original_duration;
            let unsnapped =
                (drag.original_start + pointer_delta).clamp(0.0, (end - minimum_duration).max(0.0));
            let (start, guide) =
                snap_timeline_boundary(unsnapped, drag.emitter, session, snap, view, canvas_width);
            drag.current_start = start.clamp(0.0, end - minimum_duration);
            drag.current_duration = end - drag.current_start;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimEnd => {
            let unsnapped = (drag.original_start + drag.original_duration + pointer_delta)
                .clamp(drag.original_start + minimum_duration, effect_duration);
            let (end, guide) =
                snap_timeline_boundary(unsnapped, drag.emitter, session, snap, view, canvas_width);
            let end = end.clamp(drag.original_start + minimum_duration, effect_duration);
            drag.current_start = drag.original_start;
            drag.current_duration = end - drag.original_start;
            *snap_guide = guide;
        }
    }
}
