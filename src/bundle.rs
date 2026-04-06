//! Embedded asset bundle.
//!
//! Fonts are packed into `assets/fonts/fonts.tar.zst` and symbols into
//! `assets/symbols/symbols.tar.zst` (both committed to the repo) and embedded
//! directly into the binary via `include_bytes!`. The card template is
//! embedded the same way from `template.png`.
//!
//! All asset data is extracted lazily on first access and cached for the
//! lifetime of the process.

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

const FONTS_BUNDLE: &[u8] = include_bytes!("../assets/fonts/fonts.tar.zst");
const SYMBOLS_BUNDLE: &[u8] = include_bytes!("../assets/symbols/symbols.tar.zst");
pub const TEMPLATE: &[u8] = include_bytes!("../template.png");

static FONTS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
static SYMBOLS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();

/// Decompress a zstd-compressed tar archive into a name → bytes map.
/// The map key is the tar entry path with its file extension stripped.
fn unpack_tar_zst(bundle: &[u8]) -> HashMap<String, Vec<u8>> {
    let decoder = zstd::Decoder::new(bundle).expect("decompress bundle");
    let mut archive = tar::Archive::new(decoder);
    let mut map = HashMap::new();
    for entry in archive.entries().expect("read bundle entries") {
        let mut e = entry.expect("read bundle entry");
        let path = e.path().expect("bundle entry path");
        let stem = path
            .file_stem()
            .expect("bundle entry has stem")
            .to_str()
            .expect("bundle entry path is utf-8")
            .to_owned();
        let mut data = Vec::new();
        e.read_to_end(&mut data).expect("read bundle entry data");
        map.insert(stem, data);
    }
    map
}

fn fonts() -> &'static HashMap<String, Vec<u8>> {
    FONTS.get_or_init(|| {
        // Fonts are keyed by full filename (e.g. "Mplantin.ttf") so we cannot
        // use the generic stem-stripping helper — unpack manually.
        let decoder = zstd::Decoder::new(FONTS_BUNDLE).expect("decompress fonts bundle");
        let mut archive = tar::Archive::new(decoder);
        let mut map = HashMap::new();
        for entry in archive.entries().expect("read fonts bundle entries") {
            let mut e = entry.expect("read fonts entry");
            let name = e
                .path()
                .expect("fonts entry path")
                .to_str()
                .expect("fonts entry path is utf-8")
                .to_owned();
            let mut data = Vec::new();
            e.read_to_end(&mut data).expect("read fonts entry data");
            map.insert(name, data);
        }
        map
    })
}

/// Return the bytes of a bundled font by its filename (e.g. `"Mplantin.ttf"`).
/// Panics if the name is not present — this indicates a broken build.
pub fn font(name: &str) -> &'static [u8] {
    fonts()
        .get(name)
        .unwrap_or_else(|| panic!("bundled font not found: {name}"))
}

fn symbols() -> &'static HashMap<String, Vec<u8>> {
    SYMBOLS.get_or_init(|| unpack_tar_zst(SYMBOLS_BUNDLE))
}

/// Return the SVG bytes for a bundled symbol by its bundle key (e.g. `"WU"`, `"T"`, `"HALF"`).
/// Returns `None` if the key is not present in the bundle.
pub fn symbol_svg(key: &str) -> Option<&'static [u8]> {
    symbols().get(key).map(Vec::as_slice)
}

/// Whether a symbol key exists in the bundle.
pub fn symbol_known(key: &str) -> bool {
    symbols().contains_key(key)
}
