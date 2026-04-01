//! Embedded asset bundle.
//!
//! Fonts are packed into `assets/fonts/fonts.tar.zst` (committed to the repo)
//! and embedded directly into the binary via `include_bytes!`. The card
//! template is embedded the same way from `template.png`.
//!
//! Font data is extracted lazily on first access and cached for the lifetime
//! of the process.

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

const FONTS_BUNDLE: &[u8] = include_bytes!("../assets/fonts/fonts.tar.zst");
pub const TEMPLATE: &[u8] = include_bytes!("../template.png");

static FONTS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();

fn fonts() -> &'static HashMap<String, Vec<u8>> {
    FONTS.get_or_init(|| {
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
