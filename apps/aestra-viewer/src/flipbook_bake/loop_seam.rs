//! Renders the two frames presented across a preview-loop wrap and compares its
//! image delta against the neighboring fixed-tick transitions.
use super::*;
use serde::Serialize;

const BACKGROUND: [u8; 3] = [24, 24, 28];
const DIFFERENCE_GAIN: u8 = 4;
const IMAGE_PATH: &str = "seam.png";
const REPORT_PATH: &str = "seam-report.json";
pub(super) const SEED: u64 = 0;

pub(crate) fn run(mut arguments: impl Iterator<Item = OsString>) -> Result<(), String> {
    let (Some(input), Some(output), Some(start), Some(end)) = (
        arguments.next(),
        arguments.next(),
        arguments.next(),
        arguments.next(),
    ) else {
        return Err("usage: aestra-viewer --inspect-loop INPUT.aestra.ron NEW_OUTPUT_DIRECTORY IN_FRAME OUT_FRAME [--size PIXELS] [--span UNITS] [--center X,Y,Z] [--eye X,Y,Z]".into());
    };
    let start = start
        .to_string_lossy()
        .parse()
        .map_err(|_| "IN_FRAME must be a non-negative integer")?;
    let end = end
        .to_string_lossy()
        .parse()
        .map_err(|_| "OUT_FRAME must be a non-negative integer")?;
    let purpose = CapturePurpose::LoopSeam;
    let options = BakeOptions::parse_for(arguments, purpose)?;
    let output = PathBuf::from(output);
    if output.exists() {
        return Err(format!(
            "output directory already exists: {}",
            output.display()
        ));
    }
    let prepared = prepare_capture(Path::new(&input), purpose)?;
    let duration = prepared.viewer.compiled.duration;
    let range = LoopRange::new(start, end, duration)?;
    let ticks = range.sample_ticks();
    let [red, green, blue] = BACKGROUND;
    let capture = capture_ticks(
        &prepared,
        ticks.iter().copied(),
        options,
        Color::srgb_u8(red, green, blue),
        purpose,
    )?;
    let frame_at =
        |frame| &capture.frames[ticks.binary_search(&frame).expect("recorded seam sample")];
    let last = range.last();
    let first = frame_at(range.in_frame);
    let final_frame = frame_at(last);
    let seam = Transition::compare(last, range.in_frame, final_frame, first);
    let before = (range.in_frame < last)
        .then(|| Transition::compare(last - 1, last, frame_at(last - 1), final_frame));
    let after = (range.in_frame < last).then(|| {
        Transition::compare(
            range.in_frame,
            range.in_frame + 1,
            first,
            frame_at(range.in_frame + 1),
        )
    });
    let view = options.view;
    let report = Report {
        schema_version: 1,
        effect: &prepared.viewer.compiled.name,
        tick_rate: TICK_RATE.get(),
        seed: SEED,
        range,
        size_pixels: view.size,
        world_span: view.span,
        center: view.center.to_array(),
        eye: view.eye.map(|eye| eye.to_array()),
        background_srgb: BACKGROUND,
        difference_gain: DIFFERENCE_GAIN,
        seam,
        before,
        after,
    };
    write_new_directory(&output, |directory| {
        comparison_image(final_frame, first)?
            .save(directory.join(IMAGE_PATH))
            .map_err(|error| error.to_string())?;
        let file =
            fs::File::create(directory.join(REPORT_PATH)).map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(file, &report).map_err(|error| error.to_string())
    })?;
    println!(
        "Loop seam F{} → F{} changed {} of {} pixels. Report at {}",
        seam.from_frame,
        seam.to_frame,
        seam.changed_pixels,
        seam.total_pixels,
        output.join(REPORT_PATH).display()
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct LoopRange {
    in_frame: u64,
    out_frame_exclusive: u64,
}

impl LoopRange {
    fn new(in_frame: u64, out_frame_exclusive: u64, duration: f32) -> Result<Self, String> {
        let maximum = PlaybackClock::default().maximum_frame(duration);
        if in_frame >= out_frame_exclusive || out_frame_exclusive > maximum {
            return Err(format!(
                "the loop range must satisfy 0 ≤ IN_FRAME < OUT_FRAME ≤ {maximum}"
            ));
        }
        Ok(Self {
            in_frame,
            out_frame_exclusive,
        })
    }

    fn last(self) -> u64 {
        self.out_frame_exclusive - 1
    }

    fn sample_ticks(self) -> Vec<u64> {
        let last = self.last();
        let mut ticks = vec![self.in_frame, last];
        if self.in_frame < last {
            ticks.extend([self.in_frame + 1, last - 1]);
        }
        ticks.sort_unstable();
        ticks.dedup();
        ticks
    }
}

#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    effect: &'a str,
    tick_rate: u32,
    seed: u64,
    range: LoopRange,
    size_pixels: u32,
    world_span: f32,
    center: [f32; 3],
    eye: Option<[f32; 3]>,
    background_srgb: [u8; 3],
    difference_gain: u8,
    seam: Transition,
    before: Option<Transition>,
    after: Option<Transition>,
}

#[derive(Clone, Copy, Serialize)]
struct Transition {
    from_frame: u64,
    to_frame: u64,
    changed_pixels: u64,
    total_pixels: u64,
    mean_peak_rgb_byte_delta: f64,
    peak_rgb_byte_delta: u8,
}

impl Transition {
    fn compare(from_frame: u64, to_frame: u64, from: &RgbaImage, to: &RgbaImage) -> Self {
        let total_pixels = u64::from(from.width()) * u64::from(from.height());
        let mut changed_pixels = 0;
        let mut sum = 0_u64;
        let mut peak_rgb_byte_delta = 0;
        for (a, b) in from.pixels().zip(to.pixels()) {
            let delta = pixel_delta(a, b);
            changed_pixels += u64::from(delta != 0);
            sum += u64::from(delta);
            peak_rgb_byte_delta = peak_rgb_byte_delta.max(delta);
        }
        Self {
            from_frame,
            to_frame,
            changed_pixels,
            total_pixels,
            mean_peak_rgb_byte_delta: sum as f64 / total_pixels as f64,
            peak_rgb_byte_delta,
        }
    }
}

fn pixel_delta(a: &Rgba<u8>, b: &Rgba<u8>) -> u8 {
    a[0].abs_diff(b[0])
        .max(a[1].abs_diff(b[1]))
        .max(a[2].abs_diff(b[2]))
}

fn comparison_image(last: &RgbaImage, first: &RgbaImage) -> Result<RgbaImage, String> {
    let width = last.width();
    let height = last.height();
    let sheet_width = width.checked_mul(3).ok_or("seam image width overflows")?;
    sheet_width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("seam image is too large")?;
    let mut sheet = RgbaImage::new(sheet_width, height);
    imageops::replace(&mut sheet, last, 0, 0);
    imageops::replace(&mut sheet, first, i64::from(width), 0);
    for (x, y, a) in last.enumerate_pixels() {
        let delta = pixel_delta(a, first.get_pixel(x, y)).saturating_mul(DIFFERENCE_GAIN);
        sheet.put_pixel(width * 2 + x, y, Rgba([delta, delta, delta, 255]));
    }
    Ok(sheet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_ticks_include_only_frames_inside_the_loop() {
        assert!(LoopRange::new(4, 4, 1.0).is_err());
        assert!(LoopRange::new(59, 61, 1.0).is_err());
        assert_eq!(LoopRange::new(59, 60, 1.0).unwrap().sample_ticks(), [59]);
        assert_eq!(LoopRange::new(4, 6, 1.0).unwrap().sample_ticks(), [4, 5]);
        assert_eq!(
            LoopRange::new(4, 10, 1.0).unwrap().sample_ticks(),
            [4, 5, 8, 9]
        );
    }

    #[test]
    fn report_measures_displayed_rgb_and_keeps_the_seam_direction_in_the_image() {
        let mut last = RgbaImage::from_pixel(2, 1, Rgba([24, 24, 28, 255]));
        let first = last.clone();
        let unchanged = Transition::compare(5, 2, &last, &first);
        assert_eq!(unchanged.changed_pixels, 0);
        assert_eq!(unchanged.mean_peak_rgb_byte_delta, 0.0);
        last.put_pixel(0, 0, Rgba([34, 20, 24, 255]));
        let seam = Transition::compare(5, 2, &last, &first);
        assert_eq!(seam.changed_pixels, 1);
        assert_eq!(seam.total_pixels, 2);
        assert_eq!(seam.peak_rgb_byte_delta, 10);
        assert_eq!(seam.mean_peak_rgb_byte_delta, 5.0);
        let sheet = comparison_image(&last, &first).unwrap();
        assert_eq!(sheet.dimensions(), (6, 1));
        assert_eq!(sheet.get_pixel(0, 0), last.get_pixel(0, 0));
        assert_eq!(sheet.get_pixel(2, 0), first.get_pixel(0, 0));
        assert_eq!(sheet.get_pixel(4, 0).0, [40, 40, 40, 255]);
        assert_eq!(sheet.get_pixel(5, 0).0, [0, 0, 0, 255]);
    }
}
