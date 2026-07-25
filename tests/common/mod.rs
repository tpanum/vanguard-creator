//! Shared machinery for the "can we re-generate a true original Vanguard?"
//! accuracy suite.
//!
//! The suite renders one card element at a time onto a blank canvas, compares
//! the result against a reference mask cut from a scan of the original card,
//! and scores the overlap of the text pixels (F1).
//!
//! A bare F1 number tells you *that* something is wrong but not *what*. So on
//! top of the score this module computes a **diagnosis**: how far the render is
//! translated from the reference, how much bigger or smaller it is, how much
//! ink it lays down, and what the F1 would be if the translation and scale were
//! corrected. That decomposition turns "F1 is 48%" into "you are 3 px right and
//! 2 px high; fix that and F1 becomes 71% — the remaining 29% is glyph shape".

#![allow(dead_code)]

use ab_glyph::FontRef;
use image::{ImageBuffer, Luma, RgbaImage};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use vgc::{
    card::CardDef,
    fonts::Fonts,
    layout::{Layout, DEFAULT},
    render,
};

pub mod refmask;

pub type BinMask = ImageBuffer<Luma<u8>, Vec<u8>>;

// ── Per-element render helpers ────────────────────────────────────────────────
// Each helper draws exactly one text element onto a blank 718×1024 canvas so
// that every rendered pixel belongs to the element under test. This gives
// meaningful precision when compared against per-region reference masks.
// The helpers call the same per-element `render::draw_*` functions that
// `render::render_card` itself uses, so they exercise the production path.

pub fn blank_canvas() -> RgbaImage {
    RgbaImage::from_pixel(718, 1024, image::Rgba([255, 255, 255, 255]))
}

pub fn flatten_alpha(img: &mut RgbaImage) {
    for p in img.pixels_mut() {
        let a = p[3] as f32 / 255.0;
        p[0] = (p[0] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[1] = (p[1] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[2] = (p[2] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[3] = 255;
    }
}

pub fn load_card(yaml_path: &str) -> CardDef {
    let mut card = CardDef::load(Path::new(yaml_path)).expect("load yaml");
    card.flavor = None;
    card
}

/// Everything a render depends on besides the card itself: the layout
/// constants and the two typefaces. Tests vary one field at a time to attribute
/// an F1 change to a single cause.
pub struct Ctx {
    pub layout: Layout,
    pub name_font: FontRef<'static>,
    pub body_font: FontRef<'static>,
    pub stats_font: FontRef<'static>,
}

impl Ctx {
    /// Exactly what `render_card` would use in production.
    pub fn production() -> Ctx {
        let fonts = Fonts::load().expect("fonts");
        Ctx {
            layout: DEFAULT.clone(),
            name_font: fonts.name,
            body_font: fonts.body,
            stats_font: fonts.stats,
        }
    }

    pub fn with_layout(mut self, layout: Layout) -> Ctx {
        self.layout = layout;
        self
    }
}

pub fn render_title(yaml_path: &str, ctx: &Ctx) -> RgbaImage {
    let card = load_card(yaml_path);
    let mut canvas = blank_canvas();
    render::draw_name(&mut canvas, &card.name, &ctx.name_font, &ctx.layout);
    flatten_alpha(&mut canvas);
    canvas
}

pub fn render_rules(yaml_path: &str, ctx: &Ctx) -> RgbaImage {
    let card = load_card(yaml_path);
    let mut canvas = blank_canvas();
    render::draw_rules(
        &mut canvas,
        &card.ability,
        card.flavor.as_deref(),
        &ctx.body_font,
        &ctx.layout,
    );
    flatten_alpha(&mut canvas);
    canvas
}

pub fn render_left_bubble(yaml_path: &str, ctx: &Ctx) -> RgbaImage {
    let card = load_card(yaml_path);
    let mut canvas = blank_canvas();
    render::draw_stat(
        &mut canvas,
        &card.hand,
        ctx.layout.hand_center,
        &ctx.stats_font,
        &ctx.layout,
    );
    flatten_alpha(&mut canvas);
    canvas
}

pub fn render_right_bubble(yaml_path: &str, ctx: &Ctx) -> RgbaImage {
    let card = load_card(yaml_path);
    let mut canvas = blank_canvas();
    render::draw_stat(
        &mut canvas,
        &card.life,
        ctx.layout.life_center,
        &ctx.stats_font,
        &ctx.layout,
    );
    flatten_alpha(&mut canvas);
    canvas
}

/// Parse a font file into a `'static` `FontRef` by leaking its bytes. Tests are
/// short-lived processes and the candidate set is small, so this is cheaper
/// than threading a lifetime through every helper.
pub fn load_font_file(path: &std::path::Path) -> anyhow::Result<FontRef<'static>> {
    let bytes: &'static [u8] = Box::leak(std::fs::read(path)?.into_boxed_slice());
    Ok(FontRef::try_from_slice(bytes)?)
}

// ── The case table ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Element {
    Title,
    Rules,
    LeftBubble,
    RightBubble,
}

impl Element {
    pub fn slug(self) -> &'static str {
        match self {
            Element::Title => "title",
            Element::Rules => "rules",
            Element::LeftBubble => "left_bubble",
            Element::RightBubble => "right_bubble",
        }
    }

    /// Render this element exactly as production would.
    pub fn render(self, yaml: &str) -> RgbaImage {
        self.render_with(yaml, &Ctx::production())
    }

    /// Render this element with a candidate layout/font combination — used by
    /// the calibrator and the font search to score what they are considering.
    pub fn render_with(self, yaml: &str, ctx: &Ctx) -> RgbaImage {
        match self {
            Element::Title => render_title(yaml, ctx),
            Element::Rules => render_rules(yaml, ctx),
            Element::LeftBubble => render_left_bubble(yaml, ctx),
            Element::RightBubble => render_right_bubble(yaml, ctx),
        }
    }

    /// Which typeface this element is set in.
    pub fn uses_body_font(self) -> bool {
        !matches!(self, Element::Title)
    }

    /// Which `layout::DEFAULT` fields move this element, so a measured offset
    /// can be reported as a concrete constant to edit.
    fn knobs(self) -> (&'static str, &'static str) {
        match self {
            Element::Title => ("name_center.0", "name_center.1"),
            Element::Rules => (
                "text_box.left/right",
                "rules_min_y / rules_centering_height",
            ),
            Element::LeftBubble => ("hand_center.0", "hand_center.1"),
            Element::RightBubble => ("life_center.0", "life_center.1"),
        }
    }

    /// Which field controls this element's size.
    fn size_knob(self) -> &'static str {
        match self {
            Element::Title => "name_scale",
            Element::Rules => "ability_size",
            Element::LeftBubble | Element::RightBubble => "stats_size",
        }
    }
}

pub struct Case {
    /// Card slug: names `tests/cards/<slug>.yaml` and `tests/assets/<slug>.jpg`.
    pub slug: &'static str,
    pub card: &'static str,
    pub element: Element,
    /// Minimum acceptable `(overall, shape)` F1, or `None` to score and report
    /// the case without gating it.
    pub threshold: Option<(f64, f64)>,
}

impl Case {
    pub fn label(&self) -> String {
        format!("{} {}", self.card, self.element.slug().replace('_', " "))
    }

    pub fn yaml(&self) -> String {
        format!("tests/cards/{}.yaml", self.slug)
    }

    /// The scan this case is scored against. Leaked so the mask cache can key
    /// on a `&'static str` without a lookup table.
    pub fn scan(&self) -> &'static str {
        static PATHS: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();
        let paths = PATHS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut paths = paths.lock().unwrap();
        paths.entry(self.slug).or_insert_with(|| {
            Box::leak(format!("tests/assets/{}.jpg", self.slug).into_boxed_str())
        })
    }

    pub fn fixture(&self) -> String {
        format!("{}_{}", self.slug, self.element.slug())
    }

    pub fn reference(&self) -> &'static BinMask {
        refmask::reference(self.scan(), self.element)
    }
}

/// Every card in `tests/assets/`, with the F1 floor each element must clear.
///
/// Thresholds are per (card, element) because the cards are not equally hard:
/// a two-word title on a clean scan scores far above a three-line ability with
/// a mana symbol in it. `None` leaves a case reported but ungated.
pub struct CardSpec {
    pub slug: &'static str,
    pub name: &'static str,
    /// `(overall, shape)` floors for title, rules, left bubble, right bubble.
    ///
    /// Two numbers because they fail for different reasons. **Overall** is the
    /// old score: does this element land on the card where the original's does,
    /// at the right size, in the right letterforms — everything at once.
    /// **Shape** is measured after the best translation and uniform scale have
    /// been applied, so placement is factored out and what is left is the
    /// rendering itself: the typeface, its weight, and where the lines break.
    ///
    /// Watching both separates causes that a single number confounds. A change
    /// that moves a text box shifts overall and leaves shape alone; a change of
    /// font moves shape. A drop in overall with shape steady is a layout
    /// regression, and the reverse is a rendering regression.
    pub thresholds: [Option<(f64, f64)>; 4],
}

pub const CARDS: &[CardSpec] = &[
    CardSpec {
        slug: "ashnod",
        name: "Ashnod",
        thresholds: [
            Some((0.55, 0.74)),
            Some((0.62, 0.72)),
            Some((0.62, 0.73)),
            Some((0.80, 0.80)),
        ],
    },
    CardSpec {
        slug: "crovax",
        name: "Crovax",
        thresholds: [
            Some((0.73, 0.87)),
            Some((0.39, 0.65)),
            Some((0.50, 0.74)),
            Some((0.71, 0.82)),
        ],
    },
    CardSpec {
        slug: "eladamri",
        name: "Eladamri",
        thresholds: [
            Some((0.63, 0.82)),
            Some((0.34, 0.47)),
            Some((0.28, 0.70)),
            Some((0.58, 0.74)),
        ],
    },
    CardSpec {
        slug: "ertai",
        name: "Ertai",
        thresholds: [
            Some((0.57, 0.86)),
            Some((0.38, 0.51)),
            Some((0.77, 0.81)),
            Some((0.60, 0.77)),
        ],
    },
    CardSpec {
        slug: "gerrard",
        name: "Gerrard",
        thresholds: [
            Some((0.71, 0.76)),
            Some((0.34, 0.55)),
            Some((0.34, 0.80)),
            Some((0.41, 0.82)),
        ],
    },
    CardSpec {
        slug: "hanna",
        name: "Hanna",
        thresholds: [
            Some((0.76, 0.88)),
            Some((0.30, 0.51)),
            Some((0.60, 0.77)),
            Some((0.72, 0.83)),
        ],
    },
    CardSpec {
        slug: "maraxus",
        name: "Maraxus",
        thresholds: [
            Some((0.72, 0.81)),
            Some((0.25, 0.64)),
            Some((0.65, 0.70)),
            Some((0.70, 0.76)),
        ],
    },
    CardSpec {
        slug: "mishra",
        name: "Mishra",
        thresholds: [
            Some((0.55, 0.79)),
            Some((0.41, 0.68)),
            Some((0.48, 0.68)),
            Some((0.38, 0.83)),
        ],
    },
    CardSpec {
        slug: "multani",
        name: "Multani",
        thresholds: [
            Some((0.74, 0.80)),
            Some((0.39, 0.45)),
            Some((0.70, 0.79)),
            Some((0.32, 0.83)),
        ],
    },
    CardSpec {
        slug: "oracle",
        name: "Oracle",
        thresholds: [
            Some((0.75, 0.81)),
            Some((0.35, 0.52)),
            Some((0.56, 0.77)),
            Some((0.48, 0.70)),
        ],
    },
    CardSpec {
        slug: "orim",
        name: "Orim",
        thresholds: [
            Some((0.54, 0.82)),
            Some((0.36, 0.49)),
            Some((0.65, 0.76)),
            Some((0.57, 0.76)),
        ],
    },
    CardSpec {
        slug: "rofellos",
        name: "Rofellos",
        thresholds: [
            Some((0.68, 0.73)),
            Some((0.44, 0.52)),
            Some((0.14, 0.81)),
            Some((0.29, 0.73)),
        ],
    },
    CardSpec {
        slug: "selenia",
        name: "Selenia",
        thresholds: [
            Some((0.31, 0.74)),
            Some((0.31, 0.35)),
            Some((0.56, 0.71)),
            Some((0.61, 0.77)),
        ],
    },
    CardSpec {
        slug: "serra",
        name: "Serra",
        thresholds: [
            Some((0.61, 0.77)),
            Some((0.37, 0.58)),
            Some((0.55, 0.72)),
            Some((0.68, 0.75)),
        ],
    },
    CardSpec {
        slug: "sidarkondo",
        name: "Sidar Kondo",
        thresholds: [
            Some((0.49, 0.74)),
            Some((0.32, 0.50)),
            Some((0.60, 0.80)),
            Some((0.78, 0.78)),
        ],
    },
    CardSpec {
        slug: "silverqueen",
        name: "Sliver Queen, Brood Mother",
        thresholds: [
            Some((0.58, 0.69)),
            Some((0.35, 0.46)),
            Some((0.68, 0.78)),
            Some((0.57, 0.73)),
        ],
    },
    CardSpec {
        slug: "sisay",
        name: "Sisay",
        thresholds: [
            Some((0.55, 0.82)),
            Some((0.36, 0.49)),
            Some((0.71, 0.83)),
            Some((0.47, 0.79)),
        ],
    },
    CardSpec {
        slug: "starke",
        name: "Starke",
        thresholds: [
            Some((0.80, 0.80)),
            Some((0.40, 0.44)),
            Some((0.60, 0.78)),
            Some((0.54, 0.83)),
        ],
    },
    CardSpec {
        slug: "tahngarth",
        name: "Tahngarth",
        thresholds: [
            Some((0.55, 0.75)),
            Some((0.54, 0.65)),
            Some((0.77, 0.80)),
            Some((0.63, 0.74)),
        ],
    },
    CardSpec {
        slug: "takara",
        name: "Takara",
        thresholds: [
            Some((0.71, 0.79)),
            Some((0.37, 0.47)),
            Some((0.63, 0.70)),
            Some((0.77, 0.83)),
        ],
    },
    CardSpec {
        slug: "tawnos",
        name: "Tawnos",
        thresholds: [
            Some((0.69, 0.78)),
            Some((0.51, 0.58)),
            Some((0.57, 0.70)),
            Some((0.69, 0.82)),
        ],
    },
    CardSpec {
        slug: "titania",
        name: "Titania",
        thresholds: [
            Some((0.46, 0.66)),
            Some((0.42, 0.49)),
            Some((0.76, 0.80)),
            Some((0.64, 0.74)),
        ],
    },
    CardSpec {
        slug: "urza",
        name: "Urza",
        thresholds: [
            Some((0.75, 0.85)),
            Some((0.44, 0.62)),
            Some((0.50, 0.73)),
            Some((0.79, 0.79)),
        ],
    },
    CardSpec {
        slug: "volrath",
        name: "Volrath",
        thresholds: [
            Some((0.67, 0.78)),
            Some((0.37, 0.52)),
            Some((0.68, 0.78)),
            Some((0.67, 0.83)),
        ],
    },
    CardSpec {
        slug: "xantcha",
        name: "Xantcha",
        thresholds: [
            Some((0.61, 0.83)),
            Some((0.58, 0.68)),
            Some((0.73, 0.76)),
            Some((0.64, 0.65)),
        ],
    },
];

pub fn cases() -> &'static [Case] {
    static CASES: OnceLock<Vec<Case>> = OnceLock::new();
    CASES.get_or_init(|| {
        CARDS
            .iter()
            .flat_map(|c| {
                [
                    Element::Title,
                    Element::Rules,
                    Element::LeftBubble,
                    Element::RightBubble,
                ]
                .into_iter()
                .enumerate()
                .map(move |(i, element)| Case {
                    slug: c.slug,
                    card: c.name,
                    element,
                    threshold: c.thresholds[i],
                })
            })
            .collect()
    })
}

pub fn case(slug: &str, element: Element) -> &'static Case {
    cases()
        .iter()
        .find(|c| c.slug == slug && c.element == element)
        .unwrap_or_else(|| panic!("no case for {slug}/{element:?}"))
}

// ── Binarization ──────────────────────────────────────────────────────────────

/// Convert RGBA → binary luma mask (0 = text, 255 = background).
pub fn to_binary(img: &RgbaImage, threshold: u8, text_is_dark: bool) -> BinMask {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let a = p[3] as f32 / 255.0;
        let r = p[0] as f32 * a + 255.0 * (1.0 - a);
        let g = p[1] as f32 * a + 255.0 * (1.0 - a);
        let b = p[2] as f32 * a + 255.0 * (1.0 - a);
        let l = (r * 0.299 + g * 0.587 + b * 0.114) as u8;
        let is_text = if text_is_dark {
            l < threshold
        } else {
            l > threshold
        };
        Luma([if is_text { 0 } else { 255 }])
    })
}

// ── Scoring ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default)]
pub struct Score {
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

fn score_counts(tp: u64, fp: u64, fn_: u64) -> Score {
    let precision = if tp + fp > 0 {
        tp as f64 / (tp + fp) as f64
    } else {
        0.0
    };
    let recall = if tp + fn_ > 0 {
        tp as f64 / (tp + fn_) as f64
    } else {
        0.0
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    Score {
        precision,
        recall,
        f1,
    }
}

/// Precision, recall, and F1 of text pixels (0) in `got` vs `reference`.
pub fn text_f1(got: &BinMask, reference: &BinMask) -> (f64, f64, f64) {
    assert_eq!(
        got.dimensions(),
        reference.dimensions(),
        "dimension mismatch"
    );
    let (mut tp, mut fp, mut fn_) = (0u64, 0u64, 0u64);
    for (g, r) in got.pixels().zip(reference.pixels()) {
        match (g[0] == 0, r[0] == 0) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, true) => fn_ += 1,
            (false, false) => {}
        }
    }
    let s = score_counts(tp, fp, fn_);
    (s.precision, s.recall, s.f1)
}

// ── Ink geometry ──────────────────────────────────────────────────────────────

/// The set of text pixels in a binary mask, plus its summary geometry.
#[derive(Clone)]
pub struct Ink {
    pub points: Vec<(i32, i32)>,
    pub set: HashSet<(i32, i32)>,
    pub bbox: Option<BBox>,
    pub centroid: Option<(f64, f64)>,
}

#[derive(Clone, Copy, Debug)]
pub struct BBox {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl BBox {
    pub fn w(&self) -> i32 {
        self.x1 - self.x0 + 1
    }
    pub fn h(&self) -> i32 {
        self.y1 - self.y0 + 1
    }
}

impl Ink {
    pub fn of(mask: &BinMask) -> Ink {
        let (w, h) = mask.dimensions();
        let mut points = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if mask.get_pixel(x, y)[0] == 0 {
                    points.push((x as i32, y as i32));
                }
            }
        }
        let bbox = points.iter().fold(None, |acc: Option<BBox>, &(x, y)| {
            Some(match acc {
                None => BBox {
                    x0: x,
                    y0: y,
                    x1: x,
                    y1: y,
                },
                Some(b) => BBox {
                    x0: b.x0.min(x),
                    y0: b.y0.min(y),
                    x1: b.x1.max(x),
                    y1: b.y1.max(y),
                },
            })
        });
        let centroid = if points.is_empty() {
            None
        } else {
            let n = points.len() as f64;
            Some((
                points.iter().map(|p| p.0 as f64).sum::<f64>() / n,
                points.iter().map(|p| p.1 as f64).sum::<f64>() / n,
            ))
        };
        let set = points.iter().copied().collect();
        Ink {
            points,
            set,
            bbox,
            centroid,
        }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// F1 of `got` shifted by (dx, dy) and scaled by `s` about the reference
/// centroid, against `reference`.
fn score_transformed(got: &Ink, reference: &Ink, dx: i32, dy: i32, s: f64) -> Score {
    let Some((cx, cy)) = reference.centroid else {
        return Score::default();
    };
    let mut moved: HashSet<(i32, i32)> = HashSet::with_capacity(got.len());
    for &(x, y) in &got.points {
        let nx = (cx + (x as f64 + dx as f64 - cx) * s).round() as i32;
        let ny = (cy + (y as f64 + dy as f64 - cy) * s).round() as i32;
        moved.insert((nx, ny));
    }
    let tp = moved.iter().filter(|p| reference.set.contains(p)).count() as u64;
    let fp = moved.len() as u64 - tp;
    let fn_ = reference.len() as u64 - tp;
    score_counts(tp, fp, fn_)
}

/// Best rigid translation of `got` onto `reference` within ±`radius` px.
fn best_shift(got: &Ink, reference: &Ink, radius: i32) -> (i32, i32, Score) {
    let mut best = (0, 0, Score::default());
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let s = score_transformed(got, reference, dx, dy, 1.0);
            if s.f1 > best.2.f1 {
                best = (dx, dy, s);
            }
        }
    }
    best
}

/// Best uniform scale about the reference centroid, after the given shift.
fn best_scale(got: &Ink, reference: &Ink, dx: i32, dy: i32) -> (f64, Score) {
    let mut best = (1.0f64, score_transformed(got, reference, dx, dy, 1.0));
    for step in -16i32..=16 {
        let s = 1.0 + step as f64 * 0.01;
        let sc = score_transformed(got, reference, dx, dy, s);
        if sc.f1 > best.1.f1 {
            best = (s, sc);
        }
    }
    best
}

// ── Diagnosis ─────────────────────────────────────────────────────────────────

/// A full explanation of *how* a render differs from its reference, not just
/// by how much.
pub struct Diagnosis {
    pub raw: Score,
    /// Best translation found, and the F1 it would achieve.
    pub shift: (i32, i32),
    pub shifted: Score,
    /// Best uniform scale on top of that translation.
    pub scale: f64,
    pub aligned: Score,
    pub ink_got: usize,
    pub ink_ref: usize,
    pub bbox_got: Option<BBox>,
    pub bbox_ref: Option<BBox>,
    pub centroid_delta: Option<(f64, f64)>,
    /// Per-text-line comparison (reference line ↔ rendered line).
    pub lines: Vec<LinePair>,
}

pub struct LinePair {
    pub index: usize,
    pub reference: Option<Band>,
    pub got: Option<Band>,
}

#[derive(Clone, Copy, Debug)]
pub struct Band {
    pub y0: u32,
    pub y1: u32,
    pub x0: u32,
    pub x1: u32,
    pub centroid_x: u32,
}

impl Band {
    pub fn w(&self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }
    pub fn mid_y(&self) -> u32 {
        (self.y0 + self.y1) / 2
    }
}

impl Diagnosis {
    pub fn new(got: &BinMask, reference: &BinMask) -> Diagnosis {
        let g = Ink::of(got);
        let r = Ink::of(reference);

        let (p, rc, f1) = text_f1(got, reference);
        let raw = Score {
            precision: p,
            recall: rc,
            f1,
        };

        let (dx, dy, shifted) = best_shift(&g, &r, 12);
        let (scale, aligned) = best_scale(&g, &r, dx, dy);

        let centroid_delta = match (g.centroid, r.centroid) {
            (Some(a), Some(b)) => Some((a.0 - b.0, a.1 - b.1)),
            _ => None,
        };

        Diagnosis {
            raw,
            shift: (dx, dy),
            shifted,
            scale,
            aligned,
            ink_got: g.len(),
            ink_ref: r.len(),
            bbox_got: g.bbox,
            bbox_ref: r.bbox,
            centroid_delta,
            lines: pair_bands(&text_bands(reference, 8), &text_bands(got, 8)),
        }
    }

    /// How much of the remaining error is *not* explainable by position or
    /// size — i.e. genuine glyph-shape, weight, or line-breaking mismatch.
    pub fn shape_error(&self) -> f64 {
        1.0 - self.aligned.f1
    }

    /// Human-readable, actionable report.
    pub fn report(&self, case: &Case) -> String {
        let mut out = String::new();
        let (kx, ky) = case.element.knobs();
        let pct = |v: f64| v * 100.0;

        let _ = writeln!(
            out,
            "{:<28} overall F1 {:5.1}%   shape F1 {:5.1}%   (P {:5.1}%  R {:5.1}%)",
            case.label(),
            pct(self.raw.f1),
            pct(self.aligned.f1),
            pct(self.raw.precision),
            pct(self.raw.recall),
        );

        if self.ink_ref == 0 {
            let _ = writeln!(out, "    reference mask is empty — check the fixture");
            return out;
        }
        if self.ink_got == 0 {
            let _ = writeln!(out, "    nothing was rendered — check the render helper");
            return out;
        }

        let ink_ratio = self.ink_got as f64 / self.ink_ref as f64;
        let _ = writeln!(
            out,
            "    ink        {} px rendered vs {} px reference  ({:+.1}% {})",
            self.ink_got,
            self.ink_ref,
            (ink_ratio - 1.0) * 100.0,
            if ink_ratio > 1.0 {
                "heavier"
            } else {
                "lighter"
            },
        );

        if let (Some(g), Some(r)) = (self.bbox_got, self.bbox_ref) {
            let _ = writeln!(
                out,
                "    bbox       rendered  x {:>3}..{:<3} ({:>3} wide)   y {:>3}..{:<3} ({:>3} tall)",
                g.x0,
                g.x1,
                g.w(),
                g.y0,
                g.y1,
                g.h()
            );
            let _ = writeln!(
                out,
                "               reference x {:>3}..{:<3} ({:>3} wide)   y {:>3}..{:<3} ({:>3} tall)",
                r.x0,
                r.x1,
                r.w(),
                r.y0,
                r.y1,
                r.h()
            );
            let _ = writeln!(
                out,
                "               size ratio  w {:.3}  h {:.3}",
                g.w() as f64 / r.w() as f64,
                g.h() as f64 / r.h() as f64,
            );
        }

        if let Some((cdx, cdy)) = self.centroid_delta {
            let _ = writeln!(out, "    centroid   Δx {cdx:+.1} px   Δy {cdy:+.1} px");
        }

        let (dx, dy) = self.shift;
        let _ = writeln!(
            out,
            "    placement  best shift dx {dx:+} dy {dy:+}  →  F1 {:5.1}% ({:+.1})",
            pct(self.shifted.f1),
            pct(self.shifted.f1 - self.raw.f1),
        );
        let _ = writeln!(
            out,
            "               best scale {:.2}× on top      →  F1 {:5.1}% ({:+.1})",
            self.scale,
            pct(self.aligned.f1),
            pct(self.aligned.f1 - self.shifted.f1),
        );
        let _ = writeln!(
            out,
            "    shape      {:.1}% matched after alignment; the {:.1}% missing is \
             typeface, weight or line breaks",
            pct(self.aligned.f1),
            pct(self.shape_error()),
        );

        // Actionable suggestions, ordered by how much F1 they would recover.
        let mut fixes: Vec<String> = Vec::new();
        if dx != 0 {
            fixes.push(format!(
                "move {} by {:+} px (render sits {} of the original)",
                kx,
                -dx,
                if dx > 0 { "right" } else { "left" }
            ));
        }
        if dy != 0 {
            fixes.push(format!(
                "move {} by {:+} px (render sits {} the original)",
                ky,
                -dy,
                if dy > 0 { "below" } else { "above" }
            ));
        }
        if (self.scale - 1.0).abs() >= 0.02 {
            fixes.push(format!(
                "adjust {} by {:+.0}% (render is {})",
                case.element.size_knob(),
                (self.scale - 1.0) * 100.0,
                if self.scale > 1.0 {
                    "too small"
                } else {
                    "too big"
                }
            ));
        }
        if !fixes.is_empty() {
            let _ = writeln!(out, "    ➜ try:     {}", fixes.join("\n               "));
        }

        if self.lines.len() > 1 {
            let _ = writeln!(out, "    lines      (reference ↔ rendered)");
            for lp in &self.lines {
                let _ = writeln!(out, "      {}", describe_line(lp));
            }
        }

        out
    }
}

fn describe_line(lp: &LinePair) -> String {
    match (lp.reference, lp.got) {
        (Some(r), Some(g)) => format!(
            "{:>2}  ref y {:>4} x {:>3}..{:<3} w {:>3}   got y {:>4} x {:>3}..{:<3} w {:>3}   \
             Δy {:+3}  Δcenter {:+3}  width {:.3}×",
            lp.index + 1,
            r.mid_y(),
            r.x0,
            r.x1,
            r.w(),
            g.mid_y(),
            g.x0,
            g.x1,
            g.w(),
            g.mid_y() as i32 - r.mid_y() as i32,
            g.centroid_x as i32 - r.centroid_x as i32,
            if r.w() > 0 {
                g.w() as f64 / r.w() as f64
            } else {
                0.0
            },
        ),
        (Some(r), None) => format!(
            "{:>2}  ref y {:>4} x {:>3}..{:<3} w {:>3}   got —  (line missing: our text wraps \
             into fewer lines)",
            lp.index + 1,
            r.mid_y(),
            r.x0,
            r.x1,
            r.w()
        ),
        (None, Some(g)) => format!(
            "{:>2}  ref —                              got y {:>4} x {:>3}..{:<3} w {:>3}   \
             (extra line: our text wraps into more lines)",
            lp.index + 1,
            g.mid_y(),
            g.x0,
            g.x1,
            g.w()
        ),
        (None, None) => String::new(),
    }
}

// ── Line banding ──────────────────────────────────────────────────────────────

fn band_stats(mask: &BinMask, y_start: u32, y_end: u32, w: u32) -> Band {
    let (mut x_sum, mut x_min, mut x_max, mut px_count) = (0u64, w, 0u32, 0u64);
    for by in y_start..=y_end {
        for bx in 0..w {
            if mask.get_pixel(bx, by)[0] == 0 {
                x_sum += bx as u64;
                x_min = x_min.min(bx);
                x_max = x_max.max(bx);
                px_count += 1;
            }
        }
    }
    let centroid_x = x_sum.checked_div(px_count).map_or(w / 2, |v| v as u32);
    Band {
        y0: y_start,
        y1: y_end,
        x0: x_min,
        x1: x_max,
        centroid_x,
    }
}

/// Horizontal bands of dark (text) pixels, separated by at least `min_gap`
/// near-empty rows.
///
/// A scanned reference mask is never perfectly clean: a handful of speckles in
/// the gutter between two lines is enough to bridge them into one band if the
/// row test is `count > 0`. Rows are therefore only counted as "inked" once
/// they carry a meaningful fraction of the mask's densest row, which keeps the
/// per-line report aligned between a crisp render and a noisy scan.
pub fn text_bands(mask: &BinMask, min_gap: u32) -> Vec<Band> {
    let (w, h) = mask.dimensions();
    let row_counts: Vec<u32> = (0..h)
        .map(|y| (0..w).filter(|&x| mask.get_pixel(x, y)[0] == 0).count() as u32)
        .collect();
    let peak = row_counts.iter().copied().max().unwrap_or(0);
    let row_thresh = (peak / 8).max(1);

    let mut bands = Vec::new();
    let mut in_band = false;
    let mut band_start = 0u32;
    let mut gap_count = 0u32;

    for (y, &count) in row_counts.iter().enumerate() {
        let y = y as u32;
        if count >= row_thresh {
            if !in_band {
                band_start = y;
                in_band = true;
            }
            gap_count = 0;
        } else if in_band {
            gap_count += 1;
            if gap_count >= min_gap {
                bands.push(band_stats(mask, band_start, y - gap_count, w));
                in_band = false;
                gap_count = 0;
            }
        }
    }
    if in_band {
        bands.push(band_stats(mask, band_start, h - 1, w));
    }
    bands
}

/// Pair reference bands with rendered bands positionally, so a missing or extra
/// line is reported as such instead of silently shifting every later line.
fn pair_bands(reference: &[Band], got: &[Band]) -> Vec<LinePair> {
    let n = reference.len().max(got.len());
    (0..n)
        .map(|i| LinePair {
            index: i,
            reference: reference.get(i).copied(),
            got: got.get(i).copied(),
        })
        .collect()
}

// ── Diff image ────────────────────────────────────────────────────────────────

/// Red = missed (in mask, not in render), Green = extra (in render, not in mask), Black = correct.
pub fn save_diff(got: &BinMask, reference: &BinMask, path: &str) {
    let (w, h) = got.dimensions();
    let mut diff = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let color = match (
                got.get_pixel(x, y)[0] == 0,
                reference.get_pixel(x, y)[0] == 0,
            ) {
                (true, true) => [0, 0, 0, 255],
                (false, true) => [220, 0, 0, 255],
                (true, false) => [0, 180, 0, 255],
                (false, false) => [255, 255, 255, 255],
            };
            diff.put_pixel(x, y, image::Rgba(color));
        }
    }
    diff.save(path).expect("save diff");
}

// ── Test driver ───────────────────────────────────────────────────────────────

/// Score one case, printing a full diagnosis, and assert its threshold.
pub fn run_case(case: &Case) {
    let rendered = case.element.render(&case.yaml());
    let got_mask = to_binary(&rendered, 128, true);
    let ref_mask = case.reference();

    let diag = Diagnosis::new(&got_mask, ref_mask);
    print!("{}", diag.report(case));

    if std::env::var("UPDATE_FIXTURES").is_ok() {
        rendered
            .save(format!("tests/fixtures/{}_rendered.png", case.fixture()))
            .unwrap();
        save_diff(
            &got_mask,
            ref_mask,
            &format!("tests/fixtures/{}_diff.png", case.fixture()),
        );
    }

    if let Some((overall, shape)) = case.threshold {
        assert!(
            diag.raw.f1 >= overall,
            "{} overall F1 is {:.1}% — below threshold {:.1}%. \
             Placement, size or rendering regressed.\n{}",
            case.label(),
            diag.raw.f1 * 100.0,
            overall * 100.0,
            diag.report(case),
        );
        assert!(
            diag.aligned.f1 >= shape,
            "{} shape F1 is {:.1}% — below threshold {:.1}%. \
             The letterforms themselves regressed: this score already has the best \
             translation and scale applied, so it cannot be explained by placement.\n{}",
            case.label(),
            diag.aligned.f1 * 100.0,
            shape * 100.0,
            diag.report(case),
        );
    }
}
