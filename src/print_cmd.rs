use anyhow::{bail, Context, Result};
use image::{Rgba, RgbaImage};
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

const MM_PER_INCH: f32 = 25.4;
const DPI: f32 = 300.0;

fn mm_to_px(mm: f32) -> u32 {
    (mm * DPI / MM_PER_INCH).round() as u32
}

fn parse_page_size(s: &str) -> Result<(f32, f32)> {
    match s.to_lowercase().as_str() {
        "a4" => Ok((210.0, 297.0)),
        "letter" => Ok((216.0, 279.0)),
        other => bail!("unknown page size '{other}'. Use 'a4' or 'letter'."),
    }
}

fn parse_grid(s: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() != 2 {
        bail!("invalid grid format '{s}'. Expected COLSxROWS, e.g. 3x3.");
    }
    let cols = parts[0].parse::<u32>().context("parsing grid columns")?;
    let rows = parts[1].parse::<u32>().context("parsing grid rows")?;
    if cols == 0 || rows == 0 {
        bail!("grid dimensions must be positive");
    }
    Ok((cols, rows))
}

pub fn run(
    mut image_paths: Vec<PathBuf>,
    output: &Path,
    page_size: &str,
    grid: &str,
    margin_mm: f32,
    cut_lines: bool,
    stdin: bool,
) -> Result<()> {
    if stdin {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim().trim_matches('"');
            if !trimmed.is_empty() {
                image_paths.push(PathBuf::from(trimmed));
            }
        }
    }

    if image_paths.is_empty() {
        bail!("no input images provided");
    }

    let (page_w_mm, page_h_mm) = parse_page_size(page_size)?;
    let (cols, rows) = parse_grid(grid)?;
    let cards_per_page = cols * rows;

    let page_w_px = mm_to_px(page_w_mm);
    let page_h_px = mm_to_px(page_h_mm);
    let margin_px = mm_to_px(margin_mm) as i32;
    let gap_px = mm_to_px(5.0) as i32; // 5mm inner gap

    // Determine card cell size from page geometry
    let grid_w = page_w_px as i32 - 2 * margin_px - (cols as i32 - 1) * gap_px;
    let grid_h = page_h_px as i32 - 2 * margin_px - (rows as i32 - 1) * gap_px;
    let cell_w = (grid_w / cols as i32).max(1) as u32;
    let cell_h = (grid_h / rows as i32).max(1) as u32;

    // Load and scale all card images
    let card_imgs: Vec<RgbaImage> = image_paths
        .iter()
        .filter_map(|p| {
            match image::open(p) {
                Ok(img) => {
                    let rgba = img.into_rgba8();
                    // Fit (not cover) within cell preserving aspect ratio
                    let (iw, ih) = (rgba.width(), rgba.height());
                    let scale = (cell_w as f32 / iw as f32).min(cell_h as f32 / ih as f32);
                    let new_w = (iw as f32 * scale) as u32;
                    let new_h = (ih as f32 * scale) as u32;
                    Some(image::imageops::resize(
                        &rgba,
                        new_w,
                        new_h,
                        image::imageops::FilterType::Lanczos3,
                    ))
                }
                Err(e) => {
                    eprintln!("warning: skipping {}: {e}", p.display());
                    None
                }
            }
        })
        .collect();

    if card_imgs.is_empty() {
        bail!("no valid card images to print");
    }

    let mut pages: Vec<RgbaImage> = Vec::new();

    let total_pages = card_imgs.len().div_ceil(cards_per_page as usize);
    for page_idx in 0..total_pages {
        let mut page = RgbaImage::from_pixel(page_w_px, page_h_px, Rgba([255, 255, 255, 255]));

        let start = page_idx * cards_per_page as usize;
        let end = (start + cards_per_page as usize).min(card_imgs.len());

        for (slot, img) in card_imgs[start..end].iter().enumerate() {
            let col = (slot as u32 % cols) as i32;
            let row = (slot as u32 / cols) as i32;

            let cell_x = margin_px + col * (cell_w as i32 + gap_px);
            let cell_y = margin_px + row * (cell_h as i32 + gap_px);

            // Center image within the cell
            let offset_x = cell_x + (cell_w as i32 - img.width() as i32) / 2;
            let offset_y = cell_y + (cell_h as i32 - img.height() as i32) / 2;

            image::imageops::overlay(&mut page, img, offset_x as i64, offset_y as i64);

            if cut_lines {
                draw_crop_marks(&mut page, offset_x, offset_y, img.width(), img.height());
            }
        }

        pages.push(page);
    }

    // Save pages as a multi-page PDF using PIL-style JPEG embedding
    // For now output as individual PNGs or a multi-image TIFF
    // TODO: proper PDF output with a PDF library
    if output.extension().is_some_and(|e| e == "pdf") {
        save_as_pdf(&pages, output).context("saving PDF")?;
    } else {
        for (i, page) in pages.iter().enumerate() {
            let path = if pages.len() == 1 {
                output.to_owned()
            } else {
                let stem = output.file_stem().unwrap_or_default().to_string_lossy();
                let ext = output.extension().unwrap_or_default().to_string_lossy();
                output
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(format!("{stem}-{}.{ext}", i + 1))
            };
            crate::meta::save_with_version(page, &path)?;
            println!("Saved: {}", path.display());
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
/// Distance a crop mark keeps clear of the card, and how far it then runs, in
/// millimetres. Together they must stay inside half the gutter, or two adjacent
/// cards' marks meet in the middle of it.
const CROP_MARK_GAP_MM: f32 = 0.5;
const CROP_MARK_LEN_MM: f32 = 2.0;

/// Draw crop marks around one placed card.
///
/// The marks are hung off the *card*, not the cell it was centered in: the two
/// differ whenever the card's proportions do not match the cell's, and on A4 at
/// 3x3 that left the horizontal lines 1.7mm away from the edge they claimed to
/// mark. Cutting on them would have left a white strip on every card.
///
/// They are also marks rather than rules. Full-page lines crossed the
/// neighbouring cards, printing over artwork that is not waste; a mark stops
/// short of the card it belongs to and runs outward into the gutter, which is.
fn draw_crop_marks(page: &mut RgbaImage, x: i32, y: i32, w: u32, h: u32) {
    let color = Rgba([120u8, 120, 120, 255]);
    let gap = mm_to_px(CROP_MARK_GAP_MM) as i32;
    let len = mm_to_px(CROP_MARK_LEN_MM) as i32;
    let (pw, ph) = (page.width() as i32, page.height() as i32);

    let mut plot = |px: i32, py: i32| {
        if (0..pw).contains(&px) && (0..ph).contains(&py) {
            page.put_pixel(px as u32, py as u32, color);
        }
    };

    let (x0, y0) = (x, y);
    let (x1, y1) = (x + w as i32 - 1, y + h as i32 - 1);

    // Two marks per corner: one reaching out sideways from the trim edge, one
    // reaching out vertically, so the cut line through either is unambiguous.
    for &edge_y in &[y0, y1] {
        for d in 0..len {
            plot(x0 - gap - d, edge_y);
            plot(x1 + gap + d, edge_y);
        }
    }
    for &edge_x in &[x0, x1] {
        for d in 0..len {
            plot(edge_x, y0 - gap - d);
            plot(edge_x, y1 + gap + d);
        }
    }
}

/// Save pages as a multi-page PDF by encoding each page as JPEG and wrapping in a
/// minimal hand-crafted PDF. This avoids a heavy PDF dependency for the first version.
fn save_as_pdf(pages: &[RgbaImage], output: &Path) -> Result<()> {
    use std::io::Write;

    // Object layout: catalog=1, pages=2, then three objects per page —
    // (page_i=3+3*i, contents=4+3*i, xobj=5+3*i) — then the info dictionary.
    //
    // The content stream is an object of its own because a Page is a
    // dictionary: it cannot carry `stream`/`endstream`, and `/Contents` has to
    // be an indirect reference. Inlining it produced a file whose pages strict
    // readers refuse — poppler reported "Bad /Length attribute in stream" and
    // rendered nothing.
    let pages_dict_id = 2;
    let first_page_obj = 3;
    let objs_per_page = 3;

    // Encode each page as JPEG
    let mut jpeg_bufs: Vec<Vec<u8>> = Vec::new();
    for page in pages {
        let rgb = image::DynamicImage::ImageRgba8(page.clone()).into_rgb8();
        let mut buf = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 92);
        image::ImageEncoder::write_image(
            encoder,
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ColorType::Rgb8.into(),
        )
        .context("JPEG encoding page")?;
        jpeg_bufs.push(buf);
    }

    let n = pages.len();

    // Build PDF bytes
    let mut pdf = Vec::new();
    writeln!(pdf, "%PDF-1.4")?;

    let mut offsets: Vec<u64> = Vec::new();

    // Helper to write an object
    macro_rules! write_obj {
        ($id:expr, $content:expr) => {{
            offsets.push(pdf.len() as u64);
            write!(pdf, "{} 0 obj\n", $id)?;
            pdf.extend_from_slice($content);
            write!(pdf, "\nendobj\n")?;
        }};
    }

    // Object 1: Catalog
    write_obj!(
        1,
        format!("<< /Type /Catalog /Pages {} 0 R >>", pages_dict_id).as_bytes()
    );

    // Object 2: Pages dictionary — kids listed by id
    let page_ids_str: String = (0..n)
        .map(|i| format!("{} 0 R", first_page_obj + i * objs_per_page))
        .collect::<Vec<_>>()
        .join(" ");
    write_obj!(
        2,
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", page_ids_str, n).as_bytes()
    );

    for (i, (page, jpeg)) in pages.iter().zip(jpeg_bufs.iter()).enumerate() {
        let page_obj_id = first_page_obj + i * objs_per_page;
        let contents_id = page_obj_id + 1;
        let xobj_id = page_obj_id + 2;

        let pw = page.width();
        let ph = page.height();
        // Page size in pt at 300 DPI: px / 300 * 72 pt/in
        let pt_w = pw as f32 * 72.0 / 300.0;
        let pt_h = ph as f32 * 72.0 / 300.0;

        let page_stream = format!("q {} 0 0 {} 0 0 cm /Im0 Do Q", pt_w, pt_h);

        // Page object
        write_obj!(
            page_obj_id,
            format!(
                "<< /Type /Page /Parent 2 0 R \
                 /MediaBox [0 0 {:.2} {:.2}] \
                 /Resources << /XObject << /Im0 {} 0 R >> >> \
                 /Contents {} 0 R >>",
                pt_w, pt_h, xobj_id, contents_id,
            )
            .as_bytes()
        );

        // Content stream, as its own object
        write_obj!(
            contents_id,
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                page_stream.len(),
                page_stream,
            )
            .as_bytes()
        );

        // XObject (JPEG image)
        {
            let header = format!(
                "<< /Type /XObject /Subtype /Image \
                 /Width {} /Height {} \
                 /ColorSpace /DeviceRGB /BitsPerComponent 8 \
                 /Filter /DCTDecode /Length {} >>\nstream\n",
                pw,
                ph,
                jpeg.len()
            );
            let mut obj = header.into_bytes();
            obj.extend_from_slice(jpeg);
            obj.extend_from_slice(b"\nendstream");
            write_obj!(xobj_id, &obj);
        }
    }

    // Document info — records the generator version, mirroring the EXIF
    // `Software` tag embedded in raster output.
    let info_id = first_page_obj + n * objs_per_page;
    write_obj!(
        info_id,
        format!(
            "<< /Producer ({0}) /Creator ({0}) >>",
            crate::meta::software_tag()
        )
        .as_bytes()
    );

    // Cross-reference table
    let xref_offset = pdf.len() as u64;
    write!(pdf, "xref\n0 {}\n", offsets.len() + 1)?;
    writeln!(pdf, "0000000000 65535 f ")?;
    for off in &offsets {
        writeln!(pdf, "{:010} 00000 n ", off)?;
    }
    write!(
        pdf,
        "trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{}\n%%EOF\n",
        offsets.len() + 1,
        info_id,
        xref_offset
    )?;

    std::fs::write(output, &pdf).with_context(|| format!("writing {}", output.display()))?;
    println!("Saved: {} ({} pages)", output.display(), n);
    Ok(())
}
