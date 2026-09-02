//! Reusable automation-curve and gradient visualization.
//!
//! The widget owns graph scaling, antialiased rendering, fill, grid, and value projection. Domain
//! panels keep ownership of semantic key selection, dragging, validation, and commands.

use crate::theme;
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use bevy_resvg::resvg::tiny_skia::{
    self, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};
use std::{
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
};

pub(crate) const DEFAULT_HEIGHT: f32 = 72.0;
pub(crate) const MIN_HEIGHT: f32 = 52.0;
pub(crate) const MAX_HEIGHT: f32 = 220.0;
const RASTER_WIDTH: u32 = 1536;
const RASTER_HEIGHT: u32 = 192;
const SAMPLE_COUNT: usize = 384;
const CACHE_CAPACITY: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AutomationCurvePoint {
    pub(crate) time: f32,
    pub(crate) value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AutomationGradientPoint {
    pub(crate) time: f32,
    pub(crate) color: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AutomationCurveData {
    Curve {
        points: Vec<AutomationCurvePoint>,
        value_bounds: Option<(f32, f32)>,
    },
    Gradient(Vec<AutomationGradientPoint>),
}

impl AutomationCurveData {
    pub(crate) fn key_top_percent(&self, key: usize) -> f32 {
        match self {
            Self::Curve {
                points,
                value_bounds,
            } => {
                let bounds = resolved_curve_bounds(points, *value_bounds);
                points
                    .get(key)
                    .map_or(50.0, |key| curve_top_percent(key.value, bounds))
            }
            Self::Gradient(_) => 50.0,
        }
    }

    pub(crate) fn top_percent_for_value(&self, value: f32) -> Option<f32> {
        match self {
            Self::Curve {
                points,
                value_bounds,
            } => Some(curve_top_percent(
                value,
                resolved_curve_bounds(points, *value_bounds),
            )),
            Self::Gradient(_) => None,
        }
    }

    pub(crate) fn value_for_top_percent(&self, top: f32) -> Option<f32> {
        let Self::Curve {
            points,
            value_bounds,
        } = self
        else {
            return None;
        };
        let (minimum, maximum) = resolved_curve_bounds(points, *value_bounds);
        let normalized = 1.0 - ((top.clamp(0.0, 100.0) - 8.0) / 84.0);
        Some(minimum + normalized * (maximum - minimum))
    }

    fn cache_key(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        match self {
            Self::Curve {
                points,
                value_bounds,
            } => {
                0_u8.hash(&mut hasher);
                value_bounds
                    .map(|bounds| (bounds.0.to_bits(), bounds.1.to_bits()))
                    .hash(&mut hasher);
                for key in points {
                    key.time.to_bits().hash(&mut hasher);
                    key.value.to_bits().hash(&mut hasher);
                }
            }
            Self::Gradient(keys) => {
                1_u8.hash(&mut hasher);
                for key in keys {
                    key.time.to_bits().hash(&mut hasher);
                    for channel in key.color {
                        channel.to_bits().hash(&mut hasher);
                    }
                }
            }
        }
        hasher.finish()
    }
}

#[derive(Component, Debug, Clone)]
pub(crate) struct AutomationCurveRaster(AutomationCurveData);

impl AutomationCurveRaster {
    pub(crate) fn data(&self) -> &AutomationCurveData {
        &self.0
    }

    pub(crate) fn set_data(&mut self, data: AutomationCurveData) {
        self.0 = data;
    }
}

#[derive(Resource, Default)]
pub(crate) struct AutomationCurveImageCache {
    images: HashMap<u64, Handle<Image>>,
    order: VecDeque<u64>,
}

fn curve_bounds(keys: &[AutomationCurvePoint]) -> (f32, f32) {
    let mut minimum = f32::INFINITY;
    let mut maximum = f32::NEG_INFINITY;
    for key in keys {
        if key.value.is_finite() {
            minimum = minimum.min(key.value);
            maximum = maximum.max(key.value);
        }
    }
    if !minimum.is_finite() || !maximum.is_finite() {
        return (0.0, 1.0);
    }
    if (maximum - minimum).abs() <= f32::EPSILON {
        let padding = maximum.abs().max(1.0) * 0.5;
        (minimum - padding, maximum + padding)
    } else {
        let padding = (maximum - minimum) * 0.12;
        (minimum - padding, maximum + padding)
    }
}

fn resolved_curve_bounds(
    keys: &[AutomationCurvePoint],
    value_bounds: Option<(f32, f32)>,
) -> (f32, f32) {
    value_bounds
        .filter(|(minimum, maximum)| {
            minimum.is_finite() && maximum.is_finite() && maximum > minimum
        })
        .unwrap_or_else(|| curve_bounds(keys))
}

fn curve_top_percent(value: f32, bounds: (f32, f32)) -> f32 {
    let normalized = ((value - bounds.0) / (bounds.1 - bounds.0).max(f32::EPSILON)).clamp(0.0, 1.0);
    8.0 + (1.0 - normalized) * 84.0
}

fn sample_curve(keys: &[AutomationCurvePoint], time: f32) -> f32 {
    let Some(first) = keys.first() else {
        return 0.0;
    };
    if time <= first.time {
        return first.value;
    }
    for pair in keys.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        if time <= right.time {
            let span = (right.time - left.time).max(f32::EPSILON);
            let x = ((time - left.time) / span).clamp(0.0, 1.0);
            let smooth = x * x * (3.0 - 2.0 * x);
            return left.value + (right.value - left.value) * smooth;
        }
    }
    keys.last().map_or(0.0, |key| key.value)
}

fn sample_gradient(keys: &[AutomationGradientPoint], time: f32) -> [f32; 4] {
    let Some(first) = keys.first() else {
        return [1.0; 4];
    };
    if time <= first.time {
        return first.color;
    }
    for pair in keys.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        if time <= right.time {
            let x =
                ((time - left.time) / (right.time - left.time).max(f32::EPSILON)).clamp(0.0, 1.0);
            return std::array::from_fn(|channel| {
                left.color[channel] + (right.color[channel] - left.color[channel]) * x
            });
        }
    }
    keys.last().map_or([1.0; 4], |key| key.color)
}

pub(crate) fn spawn_automation_curve(
    parent: &mut ChildSpawnerCommands,
    data: &AutomationCurveData,
) {
    parent.spawn((
        AutomationCurveRaster(data.clone()),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        Pickable::IGNORE,
    ));
}

pub(crate) fn rasterize_automation_curves(
    mut commands: Commands,
    requests: Query<
        (Entity, &AutomationCurveRaster, Option<&mut ImageNode>),
        Or<(Added<AutomationCurveRaster>, Changed<AutomationCurveRaster>)>,
    >,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<AutomationCurveImageCache>,
) {
    for (entity, request, image_node) in requests {
        let key = request.0.cache_key();
        let handle = if let Some(handle) = cache.images.get(&key) {
            handle.clone()
        } else {
            let handle = images.add(render_image(&request.0));
            cache.images.insert(key, handle.clone());
            cache.order.push_back(key);
            while cache.order.len() > CACHE_CAPACITY {
                let Some(oldest) = cache.order.pop_front() else {
                    break;
                };
                cache.images.remove(&oldest);
            }
            handle
        };
        if let Some(mut image_node) = image_node {
            image_node.image = handle;
            image_node.image_mode = NodeImageMode::Stretch;
        } else {
            commands
                .entity(entity)
                .insert(ImageNode::new(handle).with_mode(NodeImageMode::Stretch));
        }
    }
}

fn render_image(data: &AutomationCurveData) -> Image {
    let mut pixmap =
        Pixmap::new(RASTER_WIDTH, RASTER_HEIGHT).expect("valid automation raster size");
    match data {
        AutomationCurveData::Curve {
            points,
            value_bounds,
        } => render_curve(&mut pixmap, points, *value_bounds),
        AutomationCurveData::Gradient(keys) => render_gradient(&mut pixmap, keys),
    }
    let mut rgba = Vec::with_capacity((RASTER_WIDTH * RASTER_HEIGHT * 4) as usize);
    for pixel in pixmap.pixels() {
        let pixel = pixel.demultiply();
        rgba.extend_from_slice(&[pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]);
    }
    let mut image = Image::new(
        Extent3d {
            width: RASTER_WIDTH,
            height: RASTER_HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

fn paint(color: Color) -> Paint<'static> {
    let [red, green, blue, alpha] = color.to_srgba().to_u8_array();
    let mut paint = Paint::default();
    paint.set_color_rgba8(red, green, blue, alpha);
    paint
}

fn render_curve(
    pixmap: &mut Pixmap,
    keys: &[AutomationCurvePoint],
    value_bounds: Option<(f32, f32)>,
) {
    let width = RASTER_WIDTH as f32;
    let height = RASTER_HEIGHT as f32;
    for fraction in [0.25, 0.5, 0.75] {
        let mut path = PathBuilder::new();
        path.move_to(0.0, height * fraction);
        path.line_to(width, height * fraction);
        if let Some(path) = path.finish() {
            pixmap.stroke_path(
                &path,
                &paint(theme::BORDER.with_alpha(0.38)),
                &Stroke::default(),
                Transform::identity(),
                None,
            );
        }
    }

    let bounds = resolved_curve_bounds(keys, value_bounds);
    let mut line = PathBuilder::new();
    let mut fill = PathBuilder::new();
    fill.move_to(0.0, height);
    for sample in 0..SAMPLE_COUNT {
        let time = sample as f32 / (SAMPLE_COUNT - 1) as f32;
        let x = time * (width - 1.0);
        let y = curve_top_percent(sample_curve(keys, time), bounds) / 100.0 * height;
        if sample == 0 {
            line.move_to(x, y);
        } else {
            line.line_to(x, y);
        }
        fill.line_to(x, y);
    }
    fill.line_to(width, height);
    fill.close();
    if let Some(path) = fill.finish() {
        pixmap.fill_path(
            &path,
            &paint(theme::ACCENT.with_alpha(0.16)),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    if let Some(path) = line.finish() {
        pixmap.stroke_path(
            &path,
            &paint(theme::ACCENT.with_alpha(0.98)),
            &Stroke {
                width: 3.0,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..default()
            },
            Transform::identity(),
            None,
        );
    }
}

fn render_gradient(pixmap: &mut Pixmap, keys: &[AutomationGradientPoint]) {
    let top = (RASTER_HEIGHT as f32 * 0.16).round() as u32;
    let bottom = (RASTER_HEIGHT as f32 * 0.84).round() as u32;
    for x in 0..RASTER_WIDTH {
        let time = x as f32 / (RASTER_WIDTH - 1) as f32;
        let [red, green, blue, alpha] = sample_gradient(keys, time);
        let color = tiny_skia::ColorU8::from_rgba(
            (red.clamp(0.0, 1.0) * 255.0).round() as u8,
            (green.clamp(0.0, 1.0) * 255.0).round() as u8,
            (blue.clamp(0.0, 1.0) * 255.0).round() as u8,
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        )
        .premultiply();
        for y in top..bottom {
            pixmap.pixels_mut()[(y * RASTER_WIDTH + x) as usize] = color;
        }
    }
    for y in [top as f32, bottom.saturating_sub(1) as f32] {
        let mut path = PathBuilder::new();
        path.move_to(0.0, y);
        path.line_to(RASTER_WIDTH as f32, y);
        if let Some(path) = path.finish() {
            pixmap.stroke_path(
                &path,
                &paint(theme::BORDER_BRIGHT.with_alpha(0.7)),
                &Stroke::default(),
                Transform::identity(),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_keys_map_high_values_above_low_values_with_padding() {
        let data = AutomationCurveData::Curve {
            points: vec![
                AutomationCurvePoint {
                    time: 0.0,
                    value: 0.25,
                },
                AutomationCurvePoint {
                    time: 1.0,
                    value: 1.0,
                },
            ],
            value_bounds: None,
        };

        assert!(data.key_top_percent(1) < data.key_top_percent(0));
        assert!((8.0..=92.0).contains(&data.key_top_percent(0)));
        assert!((8.0..=92.0).contains(&data.key_top_percent(1)));
    }

    #[test]
    fn value_projection_round_trips_through_the_curve_area() {
        let data = AutomationCurveData::Curve {
            points: vec![
                AutomationCurvePoint {
                    time: 0.0,
                    value: -2.0,
                },
                AutomationCurvePoint {
                    time: 1.0,
                    value: 6.0,
                },
            ],
            value_bounds: None,
        };
        let top = data.top_percent_for_value(2.0).unwrap();
        assert!((data.value_for_top_percent(top).unwrap() - 2.0).abs() < 0.0001);
    }

    #[test]
    fn explicit_value_bounds_keep_normalized_curves_on_a_fixed_ordinate() {
        let data = AutomationCurveData::Curve {
            points: vec![
                AutomationCurvePoint {
                    time: 0.0,
                    value: 0.25,
                },
                AutomationCurvePoint {
                    time: 1.0,
                    value: 0.75,
                },
            ],
            value_bounds: Some((0.0, 1.0)),
        };

        assert!((data.key_top_percent(0) - 71.0).abs() < 0.0001);
        assert!((data.key_top_percent(1) - 29.0).abs() < 0.0001);
        assert!((data.value_for_top_percent(50.0).unwrap() - 0.5).abs() < 0.0001);
    }

    #[test]
    fn flat_curves_remain_centered_and_finite() {
        let keys = vec![
            AutomationCurvePoint {
                time: 0.0,
                value: 2.0,
            },
            AutomationCurvePoint {
                time: 1.0,
                value: 2.0,
            },
        ];
        let data = AutomationCurveData::Curve {
            points: keys,
            value_bounds: None,
        };

        assert_eq!(data.key_top_percent(0), 50.0);
        assert_eq!(data.key_top_percent(1), 50.0);
    }

    #[test]
    fn changed_curve_data_replaces_the_live_raster() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<AutomationCurveImageCache>()
            .add_systems(Update, rasterize_automation_curves);
        let entity = app
            .world_mut()
            .spawn(AutomationCurveRaster(AutomationCurveData::Curve {
                points: vec![
                    AutomationCurvePoint {
                        time: 0.0,
                        value: 0.0,
                    },
                    AutomationCurvePoint {
                        time: 1.0,
                        value: 1.0,
                    },
                ],
                value_bounds: None,
            }))
            .id();
        app.update();
        let first = app.world().get::<ImageNode>(entity).unwrap().image.id();

        app.world_mut()
            .get_mut::<AutomationCurveRaster>(entity)
            .unwrap()
            .set_data(AutomationCurveData::Curve {
                points: vec![
                    AutomationCurvePoint {
                        time: 0.0,
                        value: 1.0,
                    },
                    AutomationCurvePoint {
                        time: 1.0,
                        value: 0.0,
                    },
                ],
                value_bounds: None,
            });
        app.update();

        let second = app.world().get::<ImageNode>(entity).unwrap().image.id();
        assert_ne!(first, second);
    }
}
