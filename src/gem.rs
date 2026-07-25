//! The gem in the bottom bezel, and its colour.
//!
//! Every original Vanguard card carries a small glossy sphere set into the
//! bottom of the frame, and it is not always the same colour. Sampling the
//! gem disc out of all 25 scans in `tests/assets/` puts the cards into four
//! tight clusters — blue, green, red and white — so the colour is a real,
//! per-card property rather than part of the frame art. It is not the card's
//! Magic colour identity: Serra's gem is green and Volrath's is white.
//!
//! The embedded `template.png` was built from a blue card, so blue is the
//! identity here and every other colour is expressed as a transform away from
//! it. Those transforms are fitted against the scans by `examples/fit_gem.rs`
//! — see [`GemColor::transform`].
//!
//! Black is the exception. No original in the suite has a black gem, so its
//! constants are chosen to sit plausibly alongside the measured four rather
//! than derived from a card.
//!
//! The transform is applied per pixel in HSV: hue is *rotated* rather than
//! assigned, so the gem keeps its own internal hue variation, and only pixels
//! that are saturated, blue-family and inside the gem disc are touched. That
//! leaves the warm light bouncing up off the bezel into the bottom of the
//! sphere — which is the same warm colour whatever the gem is — alone.

use image::RgbaImage;
use serde::Deserialize;
use std::fmt;
use std::str::FromStr;

use crate::layout::Layout;

/// The five Magic colours a Vanguard gem can take.
///
/// Deserialization goes through [`FromStr`], so YAML accepts exactly what the
/// `validate` command accepts — including the Magic letters and any casing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(try_from = "String")]
pub enum GemColor {
    White,
    /// The colour the bundled template already carries — the identity transform.
    #[default]
    Blue,
    Black,
    Red,
    Green,
}

impl TryFrom<String> for GemColor {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl GemColor {
    pub const ALL: [GemColor; 5] = [
        GemColor::White,
        GemColor::Blue,
        GemColor::Black,
        GemColor::Red,
        GemColor::Green,
    ];

    pub fn name(self) -> &'static str {
        match self {
            GemColor::White => "white",
            GemColor::Blue => "blue",
            GemColor::Black => "black",
            GemColor::Red => "red",
            GemColor::Green => "green",
        }
    }

    /// Hue rotation, saturation multiplier and value gamma that turn the
    /// template's blue gem into this colour.
    ///
    /// These are not read off the cluster means directly — hue rotation and a
    /// value gamma are both nonlinear over the pixel distribution, so a
    /// transform built from means-of-means lands wide of the mean it was built
    /// from, and red in particular came out visibly magenta that way. They are
    /// instead *fitted*: `examples/fit_gem.rs` iterates each triple until the
    /// mean of the recoloured gem body equals the corresponding cluster mean
    /// sampled from the scans.
    ///
    /// The fit is then divided through by the fit for blue, so what is stored
    /// is each colour's offset *from the blue original* rather than from the
    /// scanner. That is what lets blue stay a strict no-op below.
    ///
    /// Cluster means over the gem body (upper two-thirds of the sphere, inside
    /// r = 11 — clear of both the specular rim and the warm bounce):
    ///
    /// | colour | mean gem RGB    | H     | S     | V     | cards |
    /// |--------|-----------------|-------|-------|-------|-------|
    /// | blue   | (58, 125, 158)  | 199.9 | 0.635 | 0.618 | 5     |
    /// | green  | (80, 131,  87)  | 127.5 | 0.387 | 0.513 | 7     |
    /// | red    | (113, 52,  65)  | 347.1 | 0.540 | 0.441 | 7     |
    /// | white  | (159, 153, 149) |  27.2 | 0.062 | 0.623 | 6     |
    fn transform(self) -> Transform {
        match self {
            // The template *is* a blue card's gem — its body mean sits within
            // a few units of the blue cluster's. Leave it exactly alone rather
            // than round-tripping it through HSV for no gain.
            GemColor::Blue => Transform {
                hue_shift: 0.0,
                sat_mul: 1.0,
                value_gamma: 1.0,
            },
            GemColor::Green => Transform {
                hue_shift: -72.7,
                sat_mul: 0.616,
                value_gamma: 1.417,
            },
            GemColor::Red => Transform {
                hue_shift: 147.2,
                sat_mul: 0.854,
                value_gamma: 1.756,
            },
            GemColor::White => Transform {
                hue_shift: -173.5,
                sat_mul: 0.099,
                value_gamma: 0.985,
            },
            // Unmeasured — no original in the suite has a black gem. A faint
            // violet cast, mostly drained of colour, with the gamma fitted to
            // put the body at V ≈ 0.28: as dark as the sphere goes and still
            // reads as a sphere rather than a hole.
            GemColor::Black => Transform {
                hue_shift: 45.0,
                sat_mul: 0.26,
                value_gamma: 2.634,
            },
        }
    }
}

impl fmt::Display for GemColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for GemColor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "white" | "w" => Ok(GemColor::White),
            "blue" | "u" => Ok(GemColor::Blue),
            "black" | "b" => Ok(GemColor::Black),
            "red" | "r" => Ok(GemColor::Red),
            "green" | "g" => Ok(GemColor::Green),
            other => Err(format!(
                "unknown gem color {other:?} (expected white, blue, black, red or green)"
            )),
        }
    }
}

/// The three knobs that turn the template's blue gem into another colour.
pub struct Transform {
    /// Degrees added to each pixel's hue. Rotating rather than assigning keeps
    /// the gem's own hue variation from light side to dark.
    pub hue_shift: f32,
    /// Multiplier on saturation — this is what drains a gem to white or black.
    pub sat_mul: f32,
    /// Exponent on value. `1.0` is a no-op and pure white is a fixed point,
    /// so the specular highlight survives any amount of darkening.
    pub value_gamma: f32,
}

/// Hue band, in degrees, that counts as "the blue of the gem". Full weight
/// between the inner pair, fading to nothing at the outer pair.
const HUE_CORE: (f32, f32) = (175.0, 255.0);
const HUE_EDGE: (f32, f32) = (150.0, 280.0);
/// Saturation below `SAT_LO` is frame metal, above `SAT_HI` is gem body.
const SAT_LO: f32 = 0.18;
const SAT_HI: f32 = 0.32;

/// Recolour the gem in place. Blue is the template's own colour and is a no-op.
///
/// Operates on the composited canvas, which is why it must run after the
/// template is laid down and before nothing in particular — no text goes
/// anywhere near the gem.
pub fn recolor(canvas: &mut RgbaImage, color: GemColor, layout: &Layout) {
    if color == GemColor::Blue {
        return;
    }
    recolor_with(canvas, &color.transform(), layout);
}

/// Apply an arbitrary transform to the gem. Exists so `examples/fit_gem.rs`
/// can search for the constants baked into [`GemColor::transform`].
pub fn recolor_with(canvas: &mut RgbaImage, t: &Transform, layout: &Layout) {
    let (cx, cy) = layout.gem_center;
    let (r_core, r_edge) = (layout.gem_radius, layout.gem_radius + 2.0);

    let x0 = (cx - r_edge).floor().max(0.0) as u32;
    let x1 = ((cx + r_edge).ceil() as u32).min(canvas.width());
    let y0 = (cy - r_edge).floor().max(0.0) as u32;
    let y1 = ((cy + r_edge).ceil() as u32).min(canvas.height());

    for y in y0..y1 {
        for x in x0..x1 {
            let dist = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let w_r = 1.0 - smoothstep(r_core, r_edge, dist);
            if w_r <= 0.0 {
                continue;
            }

            let px = canvas.get_pixel_mut(x, y);
            let (h, s, v) = rgb_to_hsv(px.0[0], px.0[1], px.0[2]);
            let w_s = smoothstep(SAT_LO, SAT_HI, s);
            let w_h = hue_weight(h);
            let w = w_r * w_s * w_h;
            if w <= 0.0 {
                continue;
            }

            let (nr, ng, nb) = hsv_to_rgb(
                (h + t.hue_shift).rem_euclid(360.0),
                (s * t.sat_mul).clamp(0.0, 1.0),
                v.powf(t.value_gamma),
            );
            px.0[0] = blend(px.0[0], nr, w);
            px.0[1] = blend(px.0[1], ng, w);
            px.0[2] = blend(px.0[2], nb, w);
        }
    }
}

fn blend(from: u8, to: u8, w: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * w)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// 1.0 inside the blue core band, tapering to 0.0 at the band edges.
fn hue_weight(h: f32) -> f32 {
    if h < HUE_CORE.0 {
        smoothstep(HUE_EDGE.0, HUE_CORE.0, h)
    } else if h > HUE_CORE.1 {
        1.0 - smoothstep(HUE_CORE.1, HUE_EDGE.1, h)
    } else {
        1.0
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Hue in degrees `[0, 360)`, saturation and value in `[0, 1]`.
fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h.rem_euclid(360.0), s, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let q = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (q(r), q(g), q(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::DEFAULT;

    fn template() -> RgbaImage {
        image::load_from_memory(crate::bundle::TEMPLATE)
            .expect("template decodes")
            .to_rgba8()
    }

    /// Mean RGB over the pixels the recolour actually reaches.
    fn gem_mean(img: &RgbaImage) -> (f32, f32, f32) {
        let (cx, cy) = DEFAULT.gem_center;
        let (mut sr, mut sg, mut sb, mut n) = (0.0, 0.0, 0.0, 0.0);
        for y in (cy - 20.0) as u32..(cy + 20.0) as u32 {
            for x in (cx - 20.0) as u32..(cx + 20.0) as u32 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                if d > DEFAULT.gem_radius {
                    continue;
                }
                let p = img.get_pixel(x, y).0;
                sr += p[0] as f32;
                sg += p[1] as f32;
                sb += p[2] as f32;
                n += 1.0;
            }
        }
        (sr / n, sg / n, sb / n)
    }

    #[test]
    fn parses_names_and_letters() {
        assert_eq!("Green".parse::<GemColor>().unwrap(), GemColor::Green);
        assert_eq!(" u ".parse::<GemColor>().unwrap(), GemColor::Blue);
        assert_eq!("B".parse::<GemColor>().unwrap(), GemColor::Black);
        assert!("purple".parse::<GemColor>().is_err());
    }

    /// YAML must accept exactly what `FromStr` does, or `vgc validate` passes
    /// a card that `vgc create` then refuses to load.
    #[test]
    fn deserializes_through_from_str() {
        let de = |s: &str| serde_yaml::from_str::<GemColor>(s);
        assert_eq!(de("red").unwrap(), GemColor::Red);
        assert_eq!(de("R").unwrap(), GemColor::Red);
        assert_eq!(de("White").unwrap(), GemColor::White);
        assert!(de("purple").is_err());
    }

    #[test]
    fn blue_is_the_identity() {
        let mut img = template();
        let before = img.clone();
        recolor(&mut img, GemColor::Blue, &DEFAULT);
        assert_eq!(img.into_raw(), before.into_raw());
    }

    #[test]
    fn recolor_touches_only_the_gem() {
        let mut img = template();
        let before = img.clone();
        recolor(&mut img, GemColor::Red, &DEFAULT);
        let (cx, cy) = DEFAULT.gem_center;
        let mut changed = 0;
        for (x, y, p) in img.enumerate_pixels() {
            if p != before.get_pixel(x, y) {
                changed += 1;
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                assert!(d <= DEFAULT.gem_radius + 2.0, "changed pixel at ({x},{y})");
            }
        }
        assert!(changed > 300, "only {changed} pixels recoloured");
    }

    /// Each colour must land in the right part of colour space — this is what
    /// catches a transform constant being typo'd into nonsense.
    #[test]
    fn each_color_lands_where_expected() {
        let cases = [
            // colour, dominant channel, and whether the gem gets lighter than blue
            (GemColor::Green, 1usize),
            (GemColor::Red, 0usize),
        ];
        for (color, dominant) in cases {
            let mut img = template();
            recolor(&mut img, color, &DEFAULT);
            let (r, g, b) = gem_mean(&img);
            let ch = [r, g, b];
            assert!(
                ch[dominant] == ch[0].max(ch[1]).max(ch[2]),
                "{color}: expected channel {dominant} to dominate, got ({r:.0},{g:.0},{b:.0})"
            );
        }

        let blue = {
            let img = template();
            gem_mean(&img)
        };
        let blue_v = blue.0.max(blue.1).max(blue.2);

        let mut white = template();
        recolor(&mut white, GemColor::White, &DEFAULT);
        let (r, g, b) = gem_mean(&white);
        let spread = r.max(g).max(b) - r.min(g).min(b);
        assert!(
            spread < 25.0,
            "white gem is not neutral: ({r:.0},{g:.0},{b:.0})"
        );

        let mut black = template();
        recolor(&mut black, GemColor::Black, &DEFAULT);
        let (r, g, b) = gem_mean(&black);
        assert!(
            r.max(g).max(b) < blue_v * 0.75,
            "black gem is not darker than blue: ({r:.0},{g:.0},{b:.0})"
        );
    }
}
