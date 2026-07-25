//! Fit the gem colour transforms in `src/gem.rs` against the original scans.
//!
//! For each colour cluster in `tests/assets/`, search for the hue shift,
//! saturation multiplier and value gamma that make the recoloured template's
//! gem body match that cluster's mean. Prints the constants, and writes a
//! sheet of every rendered gem above the original it was fitted to, so the
//! numbers can be checked by eye as well:
//!
//!   cargo run --release --example fit_gem
//!
//! The blue fit is divided out of the other three: the stored constants are
//! offsets from the blue original, not from the scanner, which is why blue
//! itself is a no-op in `GemColor::transform`.

use image::{imageops, RgbaImage};
use vgc::{
    gem::{self, GemColor, Transform},
    layout::DEFAULT,
};

/// The four gem colours present among the 25 originals, and which cards carry
/// them. Grouped by eye from a strip of every scan's gem, then confirmed by
/// how tightly each group's mean clusters. No original has a black gem.
const CLUSTERS: [(GemColor, &[&str]); 4] = [
    (
        GemColor::Blue,
        &["ertai", "gerrard", "maraxus", "sisay", "tahngarth"],
    ),
    (
        GemColor::Green,
        &[
            "ashnod", "mishra", "serra", "tawnos", "titania", "urza", "xantcha",
        ],
    ),
    (
        GemColor::Red,
        &[
            "eladamri",
            "multani",
            "oracle",
            "rofellos",
            "sidarkondo",
            "silverqueen",
            "takara",
        ],
    ),
    (
        GemColor::White,
        &["crovax", "hanna", "orim", "selenia", "starke", "volrath"],
    ),
];

/// Value the black gem's body is aimed at, since no card fixes it.
const BLACK_TARGET_V: f64 = 0.28;

const OUT: &str = "target/gem_fit.png";

fn main() {
    let template = image::load_from_memory(vgc::bundle::TEMPLATE)
        .expect("template decodes")
        .to_rgba8();

    let fits: Vec<_> = CLUSTERS
        .iter()
        .map(|(color, members)| {
            let target = cluster_mean(members);
            (*color, fit_to(&template, target), target)
        })
        .collect();

    // Express every fit relative to blue's, so the scanner's cast drops out.
    let blue = &fits[0].1;

    println!(
        "{:6} {:>21} {:>21} | {:>8} {:>7} {:>7}",
        "color", "target rgb", "fitted rgb", "hue", "sat", "gamma"
    );
    for (color, fit, target) in &fits {
        let got = body_mean(&recolored(&template, fit));
        println!(
            "{:6} ({:5.1},{:5.1},{:5.1}) ({:5.1},{:5.1},{:5.1}) | {:8.1} {:7.3} {:7.3}",
            color.name(),
            target[0],
            target[1],
            target[2],
            got[0],
            got[1],
            got[2],
            fit.hue_shift - blue.hue_shift,
            fit.sat_mul / blue.sat_mul,
            fit.value_gamma / blue.value_gamma,
        );
    }

    // Black is not fitted to anything; only its gamma is solved, for a chosen
    // body value. Its hue and saturation are a judgement call.
    let black = Transform {
        hue_shift: 45.0,
        sat_mul: 0.26,
        value_gamma: solve_gamma(&template, 45.0, 0.26, BLACK_TARGET_V),
    };
    let got = body_mean(&recolored(&template, &black));
    println!(
        "{:6} {:>21} ({:5.1},{:5.1},{:5.1}) | {:8.1} {:7.3} {:7.3}   (V target {BLACK_TARGET_V})",
        "black",
        "-- no original --",
        got[0],
        got[1],
        got[2],
        black.hue_shift,
        black.sat_mul,
        black.value_gamma,
    );

    write_sheet(&template);
    println!("\nwrote {OUT}");
}

// ── Fitting ───────────────────────────────────────────────────────────────────

/// Iterate a transform until the recoloured gem body matches `target` in HSV.
/// Each knob is corrected against the component it controls; the exponent
/// damps the step so the three do not fight each other.
fn fit_to(template: &RgbaImage, target: [f64; 3]) -> Transform {
    let (th, ts, tv) = to_hsv(target);
    let mut t = Transform {
        hue_shift: 0.0,
        sat_mul: 1.0,
        value_gamma: 1.0,
    };
    for _ in 0..80 {
        let (h, s, v) = to_hsv(body_mean(&recolored(template, &t)));
        t.hue_shift += ((((th - h + 540.0) % 360.0) - 180.0) * 0.7) as f32;
        t.sat_mul *= (ts / s.max(1e-4)).powf(0.6) as f32;
        t.value_gamma *= (tv.ln() / v.ln()).powf(0.6) as f32;
    }
    t
}

/// Solve only the value gamma, holding hue and saturation fixed.
fn solve_gamma(template: &RgbaImage, hue_shift: f32, sat_mul: f32, target_v: f64) -> f32 {
    let mut t = Transform {
        hue_shift,
        sat_mul,
        value_gamma: 1.0,
    };
    for _ in 0..60 {
        let (_, _, v) = to_hsv(body_mean(&recolored(template, &t)));
        t.value_gamma *= (target_v.ln() / v.ln()).powf(0.6) as f32;
    }
    t.value_gamma
}

fn recolored(template: &RgbaImage, t: &Transform) -> RgbaImage {
    let mut img = template.clone();
    gem::recolor_with(&mut img, t, &DEFAULT);
    img
}

// ── Sampling ──────────────────────────────────────────────────────────────────

/// Mean colour of the gem *body*: the upper two-thirds of the sphere inside
/// r = 11, which excludes the specular rim and the warm light bouncing up off
/// the bezel — neither of which carries the gem's own colour.
fn body_mean(img: &RgbaImage) -> [f64; 3] {
    sample(img, 1.0, 1.0)
}

/// The same window on a scan of arbitrary size, mapped through 718×1024
/// template coordinates.
fn scan_body_mean(img: &RgbaImage) -> [f64; 3] {
    sample(
        img,
        img.width() as f32 / 718.0,
        img.height() as f32 / 1024.0,
    )
}

fn sample(img: &RgbaImage, sx: f32, sy: f32) -> [f64; 3] {
    let (cx, cy) = DEFAULT.gem_center;
    let (mut acc, mut n) = ([0.0f64; 3], 0.0f64);
    for ty in (cy as i32 - 12)..(cy as i32 + 12) {
        for tx in (cx as i32 - 12)..(cx as i32 + 12) {
            let (dx, dy) = (tx as f32 + 0.5 - cx, ty as f32 + 0.5 - cy);
            if dx * dx + dy * dy > 121.0 || dy > 2.0 {
                continue;
            }
            let p = img
                .get_pixel((tx as f32 * sx) as u32, (ty as f32 * sy) as u32)
                .0;
            for c in 0..3 {
                acc[c] += p[c] as f64;
            }
            n += 1.0;
        }
    }
    [acc[0] / n, acc[1] / n, acc[2] / n]
}

fn cluster_mean(members: &[&str]) -> [f64; 3] {
    let mut acc = [0.0f64; 3];
    for m in members {
        let img = image::open(format!("tests/assets/{m}.jpg"))
            .unwrap_or_else(|e| panic!("opening scan for {m}: {e}"))
            .to_rgba8();
        let s = scan_body_mean(&img);
        for c in 0..3 {
            acc[c] += s[c];
        }
    }
    let n = members.len() as f64;
    [acc[0] / n, acc[1] / n, acc[2] / n]
}

fn to_hsv([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

// ── Visual check ──────────────────────────────────────────────────────────────

const CELL: u32 = 44;
const ZOOM: u32 = 8;

/// Top row: the gem as `vgc` renders it, in all five colours. Bottom row: one
/// original carrying that colour, at the same magnification. Black has no
/// original, so its cell below is left empty.
fn write_sheet(template: &RgbaImage) {
    let originals = ["volrath", "gerrard", "", "oracle", "titania"];
    let side = CELL * ZOOM;
    let mut sheet = RgbaImage::from_pixel(side * 5, side * 2, image::Rgba([24, 24, 24, 255]));
    for (i, color) in GemColor::ALL.iter().enumerate() {
        let mut img = template.clone();
        gem::recolor(&mut img, *color, &DEFAULT);
        imageops::overlay(&mut sheet, &zoom(&img, 1.0, 1.0), i as i64 * side as i64, 0);

        if originals[i].is_empty() {
            continue;
        }
        let scan = image::open(format!("tests/assets/{}.jpg", originals[i]))
            .expect("opening scan")
            .to_rgba8();
        let (sx, sy) = (scan.width() as f32 / 718.0, scan.height() as f32 / 1024.0);
        imageops::overlay(
            &mut sheet,
            &zoom(&scan, sx, sy),
            i as i64 * side as i64,
            side as i64,
        );
    }
    std::fs::create_dir_all("target").expect("creating target/");
    sheet.save(OUT).expect("writing sheet");
}

fn zoom(img: &RgbaImage, sx: f32, sy: f32) -> RgbaImage {
    let (cx, cy) = DEFAULT.gem_center;
    let half = CELL as f32 / 2.0;
    let crop = imageops::crop_imm(
        img,
        ((cx - half) * sx) as u32,
        ((cy - half) * sy) as u32,
        (CELL as f32 * sx) as u32,
        (CELL as f32 * sy) as u32,
    )
    .to_image();
    imageops::resize(
        &crop,
        CELL * ZOOM,
        CELL * ZOOM,
        imageops::FilterType::Nearest,
    )
}
