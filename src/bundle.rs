//! Embedded asset bundle: fonts and card template packed into a zstd-compressed
//! tar archive at build time and included directly into the binary.
//!
//! Assets are extracted lazily on first access and cached for the lifetime of
//! the process.

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

const DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bundled.tar.zst"));

static ASSETS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();

fn assets() -> &'static HashMap<String, Vec<u8>> {
    ASSETS.get_or_init(|| {
        let decoder = zstd::Decoder::new(DATA).expect("decompress asset bundle");
        let mut archive = tar::Archive::new(decoder);
        let mut map = HashMap::new();
        for entry in archive.entries().expect("read asset bundle entries") {
            let mut e = entry.expect("read bundle entry");
            let name = e
                .path()
                .expect("bundle entry path")
                .to_str()
                .expect("bundle entry path is utf-8")
                .to_owned();
            let mut data = Vec::new();
            e.read_to_end(&mut data).expect("read bundle entry data");
            map.insert(name, data);
        }
        map
    })
}

/// Return the bytes of a bundled asset by its archive path (e.g. `"template.png"`).
/// Panics if the name is not present — this indicates a broken build.
pub fn get(name: &str) -> &'static [u8] {
    assets()
        .get(name)
        .unwrap_or_else(|| panic!("bundled asset not found: {name}"))
}
