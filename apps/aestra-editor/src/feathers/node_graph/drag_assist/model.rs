//! Pure, bounded soft snapping in logical graph units. Never moves other nodes.
use bevy::prelude::*;

pub(super) const MAX_NODES: usize = 512;
pub(super) const MAX_PORTS: usize = 32;
const RADIUS: f32 = 6.0; // logical screen pixels, independent of canvas zoom/DPI

#[derive(Clone, Debug)]
pub(super) struct Shape {
    pub rect: Rect,
    pub rows: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Guide {
    pub axis: usize,
    pub coordinate: f32,
    pub start: f32,
    pub end: f32,
}

#[derive(Clone, Copy, Debug)]
struct Match {
    delta: f32,
    guide: Guide,
}

fn consider(best: &mut Option<Match>, delta: f32, guide: Guide, radius: f32) {
    if delta.is_finite()
        && delta.abs() <= radius
        && best.is_none_or(|old| {
            (delta.abs(), guide.coordinate, guide.start, guide.end)
                < (
                    old.delta.abs(),
                    old.guide.coordinate,
                    old.guide.start,
                    old.guide.end,
                )
        })
    {
        *best = Some(Match { delta, guide });
    }
}

/// Raw pointer placement is the input every time; snapped output is never accumulated.
pub(super) fn snap(
    raw: Vec2,
    moving: &Shape,
    targets: &[Shape],
    zoom: f32,
    grid: bool,
    alignment: bool,
) -> (Vec2, Vec<Guide>) {
    if !raw.is_finite() || !zoom.is_finite() || zoom <= 0.0 {
        return (raw, vec![]);
    }
    let radius = RADIUS / zoom;
    let size = moving.rect.size();
    let rect = Rect::from_corners(raw, raw + size);
    let mut matches = [None, None];
    if alignment && targets.len() <= MAX_NODES {
        for target in targets {
            if !target.rect.min.is_finite() || !target.rect.max.is_finite() {
                continue;
            }
            for (axis, best) in matches.iter_mut().enumerate() {
                let cross = 1 - axis;
                // Local neighbors only; do not magnetize unrelated distant branches.
                let separation = (target.rect.min[cross] - rect.max[cross])
                    .max(rect.min[cross] - target.rect.max[cross]);
                if separation * zoom > 320.0 {
                    continue;
                }
                for from in [rect.min[axis], rect.center()[axis], rect.max[axis]] {
                    for to in [
                        target.rect.min[axis],
                        target.rect.center()[axis],
                        target.rect.max[axis],
                    ] {
                        consider(
                            best,
                            to - from,
                            Guide {
                                axis,
                                coordinate: to,
                                start: rect.min[cross].min(target.rect.min[cross]),
                                end: rect.max[cross].max(target.rect.max[cross]),
                            },
                            radius,
                        );
                    }
                }
            }
            if moving.rows.len() <= MAX_PORTS
                && target.rows.len() <= MAX_PORTS
                && (target.rect.min.x - rect.max.x).max(rect.min.x - target.rect.max.x) * zoom
                    <= 320.0
            {
                for from in &moving.rows {
                    for to in &target.rows {
                        let y = target.rect.min.y + to;
                        consider(
                            &mut matches[1],
                            y - (raw.y + from),
                            Guide {
                                axis: 1,
                                coordinate: y,
                                start: rect.min.x.min(target.rect.min.x),
                                end: rect.max.x.max(target.rect.max.x),
                            },
                            radius,
                        );
                    }
                }
            }
        }
    }
    let mut result = raw;
    let mut guides = Vec::new();
    for axis in 0..2 {
        if let Some(found) = matches[axis] {
            result[axis] += found.delta;
            guides.push(found.guide);
        } else if grid {
            let nearest =
                (raw[axis] / super::super::GRID_SPACING).round() * super::super::GRID_SPACING;
            if (nearest - raw[axis]).abs() <= radius {
                result[axis] = nearest;
                guides.push(Guide {
                    axis,
                    coordinate: nearest,
                    start: rect.min[1 - axis] - 16.0,
                    end: rect.max[1 - axis] + 16.0,
                });
            }
        }
    }
    (result, guides)
}

#[cfg(test)]
mod tests;
