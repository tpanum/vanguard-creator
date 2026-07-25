//! Embedding of generator provenance into rendered card images.
//!
//! Every image `vgc` writes carries the version of the tool that produced it,
//! so a card file found later can be traced back to the exact release that
//! rendered it. The version lives in a real EXIF `Software` tag (PNG `eXIf`
//! chunk / JPEG `APP1` segment) and, for PNG, additionally in a `tEXt` chunk so
//! that plain `pnginfo`-style tools show it without EXIF support.

use anyhow::{Context, Result};
use image::RgbaImage;
use std::path::Path;

/// EXIF `Software` string written into every rendered image.
pub fn software_tag() -> String {
    format!("vgc {}", env!("CARGO_PKG_VERSION"))
}

/// Save `img` to `path`, embedding the generator version as image metadata.
///
/// PNG and JPEG outputs get an EXIF `Software` tag; any other format the
/// `image` crate supports is written as-is (no metadata container we can rely
/// on), so an unusual extension degrades to a plain save rather than an error.
pub fn save_with_version(img: &RgbaImage, path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let bytes = match ext.as_str() {
        "png" => {
            let mut buf = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                .context("encoding PNG")?;
            png_with_metadata(&buf, &software_tag())
        }
        "jpg" | "jpeg" => {
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
            let mut buf = Vec::new();
            rgb.write_to(
                &mut std::io::Cursor::new(&mut buf),
                image::ImageFormat::Jpeg,
            )
            .context("encoding JPEG")?;
            jpeg_with_metadata(&buf, &software_tag())
        }
        _ => {
            return img
                .save(path)
                .with_context(|| format!("saving {}", path.display()))
        }
    };

    std::fs::write(path, bytes).with_context(|| format!("saving {}", path.display()))
}

/// Build a minimal little-endian TIFF/EXIF block holding a single `Software`
/// (tag 0x0131) ASCII entry.
pub fn exif_block(software: &str) -> Vec<u8> {
    // NUL-terminated, per the EXIF ASCII type.
    let mut value: Vec<u8> = software.as_bytes().to_vec();
    value.push(0);

    let mut out = Vec::new();
    out.extend_from_slice(b"II\x2a\x00"); // little-endian TIFF magic
    out.extend_from_slice(&8u32.to_le_bytes()); // offset of IFD0
    out.extend_from_slice(&1u16.to_le_bytes()); // one entry
    out.extend_from_slice(&0x0131u16.to_le_bytes()); // Software
    out.extend_from_slice(&2u16.to_le_bytes()); // type ASCII
    out.extend_from_slice(&(value.len() as u32).to_le_bytes()); // count

    // Values of 4 bytes or fewer are inlined; longer ones are referenced by
    // offset from the start of the TIFF header.
    if value.len() <= 4 {
        let mut inline = [0u8; 4];
        inline[..value.len()].copy_from_slice(&value);
        out.extend_from_slice(&inline);
        out.extend_from_slice(&0u32.to_le_bytes()); // no IFD1
    } else {
        let value_offset = 8 + 2 + 12 + 4; // header + count + entry + next-IFD link
        out.extend_from_slice(&(value_offset as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // no IFD1
        out.extend_from_slice(&value);
    }
    out
}

/// Insert `eXIf` and `tEXt` chunks carrying `software` into an encoded PNG.
///
/// Both chunks go immediately after `IHDR`, which is a valid position for
/// either. Returns the input unchanged if it does not look like a PNG.
fn png_with_metadata(png: &[u8], software: &str) -> Vec<u8> {
    const SIG_LEN: usize = 8;
    // IHDR is always the first chunk: 4 length + 4 type + 13 data + 4 CRC.
    const IHDR_END: usize = SIG_LEN + 25;

    if png.len() < IHDR_END || png[..SIG_LEN] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return png.to_vec();
    }

    let mut text = b"Software\0".to_vec();
    text.extend_from_slice(software.as_bytes());

    let mut out = Vec::with_capacity(png.len() + text.len() + software.len() + 64);
    out.extend_from_slice(&png[..IHDR_END]);
    push_png_chunk(&mut out, b"eXIf", &exif_block(software));
    push_png_chunk(&mut out, b"tEXt", &text);
    out.extend_from_slice(&png[IHDR_END..]);
    out
}

fn push_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);

    let mut hasher = crc32fast::Hasher::new();
    hasher.update(kind);
    hasher.update(data);
    out.extend_from_slice(&hasher.finalize().to_be_bytes());
}

/// Insert an EXIF `APP1` segment carrying `software` into an encoded JPEG.
///
/// The segment goes directly after the `SOI` marker. Returns the input
/// unchanged if it does not look like a JPEG or if the EXIF block would
/// overflow a segment.
fn jpeg_with_metadata(jpeg: &[u8], software: &str) -> Vec<u8> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return jpeg.to_vec();
    }

    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&exif_block(software));

    // Segment length covers the two length bytes themselves.
    let Ok(seg_len) = u16::try_from(payload.len() + 2) else {
        return jpeg.to_vec();
    };

    let mut out = Vec::with_capacity(jpeg.len() + payload.len() + 4);
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&seg_len.to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&jpeg[2..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locate a top-level PNG chunk by type, returning its data.
    fn png_chunk<'a>(png: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
        let mut pos = 8;
        while pos + 8 <= png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let ty = &png[pos + 4..pos + 8];
            let data = png.get(pos + 8..pos + 8 + len)?;
            if ty == kind {
                return Some(data);
            }
            pos += 12 + len;
        }
        None
    }

    #[test]
    fn exif_block_holds_software_tag() {
        let block = exif_block("vgc 1.2.3");
        assert_eq!(&block[..4], b"II\x2a\x00");
        // Value is longer than 4 bytes, so it is stored at the trailing offset.
        assert!(block.ends_with(b"vgc 1.2.3\0"));
    }

    #[test]
    fn png_carries_version_metadata() {
        let img = RgbaImage::new(2, 2);
        let mut encoded = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut encoded),
            image::ImageFormat::Png,
        )
        .unwrap();

        let tagged = png_with_metadata(&encoded, &software_tag());

        let exif = png_chunk(&tagged, b"eXIf").expect("eXIf chunk");
        assert_eq!(exif, exif_block(&software_tag()));

        let text = png_chunk(&tagged, b"tEXt").expect("tEXt chunk");
        assert_eq!(
            text,
            [b"Software\0".as_slice(), software_tag().as_bytes()].concat()
        );

        // The result must still decode as a PNG, and a real EXIF-aware decoder
        // must find the block we wrote.
        let mut decoder =
            image::codecs::png::PngDecoder::new(std::io::Cursor::new(&tagged)).expect("re-decode");
        assert_eq!(image::ImageDecoder::dimensions(&decoder), (2, 2));
        let exif = image::ImageDecoder::exif_metadata(&mut decoder)
            .unwrap()
            .expect("decoder exposes EXIF");
        assert_eq!(exif, exif_block(&software_tag()));
    }

    #[test]
    fn jpeg_carries_exif_segment() {
        let jpeg = [0xFFu8, 0xD8, 0xFF, 0xD9];
        let tagged = jpeg_with_metadata(&jpeg, "vgc 1.2.3");
        assert_eq!(&tagged[..2], &[0xFF, 0xD8]);
        assert_eq!(&tagged[2..4], &[0xFF, 0xE1]);
        let len = u16::from_be_bytes([tagged[4], tagged[5]]) as usize;
        assert_eq!(&tagged[6..10], b"Exif");
        assert_eq!(len + 4, tagged.len() - 2); // segment + SOI + EOI
    }

    #[test]
    fn unrecognised_input_passes_through() {
        assert_eq!(png_with_metadata(b"not a png", "vgc"), b"not a png");
        assert_eq!(jpeg_with_metadata(b"not a jpeg", "vgc"), b"not a jpeg");
    }
}
