use image::{Rgba, RgbaImage, imageops};
use std::{fs, path::Path};

const ANALYSIS_SCALE: u32 = 4;
const FOREGROUND_THRESHOLD: f32 = 4.0;
const MAX_FOREGROUND_RMSE: f32 = 0.15;
const MAX_DIFFERING_FRACTION: f32 = 0.55;
const MIN_COVERAGE_RATIO: f32 = 0.55;
const MAX_COVERAGE_RATIO: f32 = 1.80;
const MAX_CENTROID_DRIFT: f32 = 24.0;

#[derive(Debug)]
pub(crate) struct ComparisonReport {
    pub(crate) frames: Vec<FrameMetrics>,
}

#[derive(Debug)]
pub(crate) struct FrameMetrics {
    pub(crate) index: usize,
    pub(crate) foreground_rmse: f32,
    pub(crate) differing_fraction: f32,
    pub(crate) coverage_ratio: f32,
    pub(crate) centroid_drift: f32,
    pub(crate) passed: bool,
}

pub(crate) fn compare_capture(
    reference_directory: &Path,
    actual_directory: &Path,
    frame_count: usize,
) -> Result<ComparisonReport, String> {
    let mut frames = Vec::with_capacity(frame_count);
    for index in 0..frame_count {
        let file_name = format!("frame-{index:03}.png");
        let reference_path = reference_directory.join(&file_name);
        let actual_path = actual_directory.join(&file_name);
        let reference = image::open(&reference_path)
            .map_err(|error| format!("could not load {}: {error}", reference_path.display()))?
            .to_rgba8();
        let actual = image::open(&actual_path)
            .map_err(|error| format!("could not load {}: {error}", actual_path.display()))?
            .to_rgba8();
        let metrics = compare_images(index, &reference, &actual)?;
        difference_image(&reference, &actual)
            .save(actual_directory.join(format!("diff-{index:03}.png")))
            .map_err(|error| format!("could not save frame {index} difference: {error}"))?;
        frames.push(metrics);
    }

    let report = ComparisonReport { frames };
    let report_path = actual_directory.join("regression-report.md");
    fs::write(&report_path, report.to_markdown())
        .map_err(|error| format!("could not write {}: {error}", report_path.display()))?;
    let failures = report.frames.iter().filter(|frame| !frame.passed).count();
    if failures > 0 {
        return Err(format!(
            "visual regression failed for {failures}/{frame_count} frames; see {}",
            report_path.display()
        ));
    }
    Ok(report)
}

impl ComparisonReport {
    fn to_markdown(&self) -> String {
        let mut output = format!(
            "# Aestra visual regression\n\n- Result: **{}**\n- Analysis: {}x downsampled foreground comparison\n- Limits: RMSE <= {:.2}, differing fraction <= {:.2}, coverage {:.2}..={:.2}, centroid drift <= {:.1}px\n\n| Frame | Result | RMSE | Differing | Coverage | Centroid drift | Diff |\n| ---: | :---: | ---: | ---: | ---: | ---: | :--- |\n",
            if self.frames.iter().all(|frame| frame.passed) {
                "PASS"
            } else {
                "FAIL"
            },
            ANALYSIS_SCALE,
            MAX_FOREGROUND_RMSE,
            MAX_DIFFERING_FRACTION,
            MIN_COVERAGE_RATIO,
            MAX_COVERAGE_RATIO,
            MAX_CENTROID_DRIFT,
        );
        for frame in &self.frames {
            output.push_str(&format!(
                "| {:03} | {} | {:.4} | {:.2}% | {:.3} | {:.2}px | [image](diff-{:03}.png) |\n",
                frame.index,
                if frame.passed { "PASS" } else { "FAIL" },
                frame.foreground_rmse,
                frame.differing_fraction * 100.0,
                frame.coverage_ratio,
                frame.centroid_drift,
                frame.index,
            ));
        }
        output
    }
}

fn compare_images(
    index: usize,
    reference: &RgbaImage,
    actual: &RgbaImage,
) -> Result<FrameMetrics, String> {
    if reference.dimensions() != actual.dimensions() {
        return Err(format!(
            "frame {index} dimensions differ: reference {:?}, actual {:?}",
            reference.dimensions(),
            actual.dimensions()
        ));
    }
    let width = (reference.width() / ANALYSIS_SCALE).max(1);
    let height = (reference.height() / ANALYSIS_SCALE).max(1);
    let reference = imageops::resize(reference, width, height, imageops::FilterType::Triangle);
    let actual = imageops::resize(actual, width, height, imageops::FilterType::Triangle);
    let reference_background = corner_background(&reference);
    let actual_background = corner_background(&actual);

    let mut squared_error = 0.0;
    let mut union_pixels = 0_u32;
    let mut differing_pixels = 0_u32;
    let mut reference_pixels = 0_u32;
    let mut actual_pixels = 0_u32;
    let mut reference_weight = 0.0;
    let mut actual_weight = 0.0;
    let mut reference_centroid = [0.0, 0.0];
    let mut actual_centroid = [0.0, 0.0];

    for (x, y, reference_pixel) in reference.enumerate_pixels() {
        let actual_pixel = actual.get_pixel(x, y);
        let reference_energy = color_distance(reference_pixel, reference_background);
        let actual_energy = color_distance(actual_pixel, actual_background);
        let reference_foreground = reference_energy > FOREGROUND_THRESHOLD;
        let actual_foreground = actual_energy > FOREGROUND_THRESHOLD;
        if reference_foreground {
            reference_pixels += 1;
            reference_weight += reference_energy;
            reference_centroid[0] += x as f32 * reference_energy;
            reference_centroid[1] += y as f32 * reference_energy;
        }
        if actual_foreground {
            actual_pixels += 1;
            actual_weight += actual_energy;
            actual_centroid[0] += x as f32 * actual_energy;
            actual_centroid[1] += y as f32 * actual_energy;
        }
        if !(reference_foreground || actual_foreground) {
            continue;
        }
        union_pixels += 1;
        let mut maximum_difference = 0.0_f32;
        for channel in 0..3 {
            let difference = f32::from(reference_pixel[channel]) - f32::from(actual_pixel[channel]);
            squared_error += (difference / 255.0).powi(2);
            maximum_difference = maximum_difference.max(difference.abs());
        }
        if maximum_difference > 16.0 {
            differing_pixels += 1;
        }
    }

    let foreground_rmse = if union_pixels == 0 {
        0.0
    } else {
        (squared_error / (union_pixels * 3) as f32).sqrt()
    };
    let differing_fraction = if union_pixels == 0 {
        0.0
    } else {
        differing_pixels as f32 / union_pixels as f32
    };
    let coverage_ratio = if reference_pixels == 0 {
        if actual_pixels == 0 {
            1.0
        } else {
            f32::INFINITY
        }
    } else {
        actual_pixels as f32 / reference_pixels as f32
    };
    let centroid_drift = match (reference_weight > 0.0, actual_weight > 0.0) {
        (true, true) => {
            let reference_x = reference_centroid[0] / reference_weight;
            let reference_y = reference_centroid[1] / reference_weight;
            let actual_x = actual_centroid[0] / actual_weight;
            let actual_y = actual_centroid[1] / actual_weight;
            let difference_x = reference_x - actual_x;
            let difference_y = reference_y - actual_y;
            (difference_x * difference_x + difference_y * difference_y).sqrt()
                * ANALYSIS_SCALE as f32
        }
        (false, false) => 0.0,
        _ => f32::INFINITY,
    };
    let passed = foreground_rmse <= MAX_FOREGROUND_RMSE
        && differing_fraction <= MAX_DIFFERING_FRACTION
        && (MIN_COVERAGE_RATIO..=MAX_COVERAGE_RATIO).contains(&coverage_ratio)
        && centroid_drift <= MAX_CENTROID_DRIFT;

    Ok(FrameMetrics {
        index,
        foreground_rmse,
        differing_fraction,
        coverage_ratio,
        centroid_drift,
        passed,
    })
}

fn corner_background(image: &RgbaImage) -> [f32; 3] {
    let corners = [
        image.get_pixel(0, 0),
        image.get_pixel(image.width() - 1, 0),
        image.get_pixel(0, image.height() - 1),
        image.get_pixel(image.width() - 1, image.height() - 1),
    ];
    let mut background = [0.0; 3];
    for pixel in corners {
        for channel in 0..3 {
            background[channel] += f32::from(pixel[channel]) * 0.25;
        }
    }
    background
}

fn color_distance(pixel: &Rgba<u8>, background: [f32; 3]) -> f32 {
    (0..3)
        .map(|channel| (f32::from(pixel[channel]) - background[channel]).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn difference_image(reference: &RgbaImage, actual: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(reference.width(), reference.height(), |x, y| {
        let reference = reference.get_pixel(x, y);
        let actual = actual.get_pixel(x, y);
        Rgba([
            reference[0].abs_diff(actual[0]).saturating_mul(4),
            reference[1].abs_diff(actual[1]).saturating_mul(4),
            reference[2].abs_diff(actual[2]).saturating_mul(4),
            255,
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_effect() -> RgbaImage {
        let mut image = RgbaImage::from_pixel(64, 64, Rgba([3, 4, 9, 255]));
        for y in 24..40 {
            for x in 24..40 {
                image.put_pixel(x, y, Rgba([180, 90, 240, 255]));
            }
        }
        image
    }

    #[test]
    fn identical_frames_pass() {
        let image = sample_effect();
        let metrics = compare_images(0, &image, &image).unwrap();
        assert!(metrics.passed);
        assert_eq!(metrics.foreground_rmse, 0.0);
    }

    #[test]
    fn missing_effect_fails() {
        let reference = sample_effect();
        let actual = RgbaImage::from_pixel(64, 64, Rgba([3, 4, 9, 255]));
        let metrics = compare_images(0, &reference, &actual).unwrap();
        assert!(!metrics.passed);
        assert_eq!(metrics.coverage_ratio, 0.0);
    }

    #[test]
    fn small_color_drift_is_tolerated() {
        let reference = sample_effect();
        let mut actual = reference.clone();
        for pixel in actual.pixels_mut() {
            pixel[0] = pixel[0].saturating_add(3);
        }
        assert!(compare_images(0, &reference, &actual).unwrap().passed);
    }
}
