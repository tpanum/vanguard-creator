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
use std::path::Path;
use std::sync::OnceLock;

const FONTS_BUNDLE: &[u8] = include_bytes!("../assets/fonts/fonts.tar.zst");
const SYMBOLS_BUNDLE: &[u8] = include_bytes!("../assets/symbols/symbols.tar.zst");
pub const TEMPLATE: &[u8] = include_bytes!("../template.png");

static FONTS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
static SYMBOLS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();

/// Decompress a zstd-compressed tar archive into a name → bytes map,
/// deriving each entry's map key from its path with `key_of`.
fn unpack_tar_zst(
    bundle: &[u8],
    key_of: fn(&Path) -> Option<&std::ffi::OsStr>,
) -> HashMap<String, Vec<u8>> {
    let decoder = zstd::Decoder::new(bundle).expect("decompress bundle");
    let mut archive = tar::Archive::new(decoder);
    let mut map = HashMap::new();
    for entry in archive.entries().expect("read bundle entries") {
        let mut e = entry.expect("read bundle entry");
        let path = e.path().expect("bundle entry path");
        let key = key_of(&path)
            .expect("bundle entry has key")
            .to_str()
            .expect("bundle entry path is utf-8")
            .to_owned();
        let mut data = Vec::new();
        e.read_to_end(&mut data).expect("read bundle entry data");
        map.insert(key, data);
    }
    map
}

/// Fonts are keyed by full filename (e.g. `"Mplantin.ttf"`).
fn fonts() -> &'static HashMap<String, Vec<u8>> {
    FONTS.get_or_init(|| unpack_tar_zst(FONTS_BUNDLE, Path::file_name))
}

/// Symbols are keyed by file stem (e.g. `"WU"`).
fn symbols() -> &'static HashMap<String, Vec<u8>> {
    SYMBOLS.get_or_init(|| unpack_tar_zst(SYMBOLS_BUNDLE, Path::file_stem))
}

/// Return the bytes of a bundled font by its filename (e.g. `"Mplantin.ttf"`).
/// Panics if the name is not present — this indicates a broken build.
pub fn font(name: &str) -> &'static [u8] {
    fonts()
        .get(name)
        .unwrap_or_else(|| panic!("bundled font not found: {name}"))
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
