use std::fs;
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("bundled.tar.zst");

    let entries: &[(&str, &str)] = &[
        (
            "fonts/Fremont-Regular.ttf",
            "assets/fonts/Fremont-Regular.ttf",
        ),
        ("fonts/Mplantin.ttf", "assets/fonts/Mplantin.ttf"),
        ("fonts/Mplantin-Bold.ttf", "assets/fonts/Mplantin-Bold.ttf"),
        ("template.png", "template.png"),
    ];

    let out_file = fs::File::create(&out_path).expect("create bundled.tar.zst");
    let encoder = zstd::Encoder::new(out_file, 3)
        .expect("create zstd encoder")
        .auto_finish();
    let mut tar = tar::Builder::new(encoder);

    for (archive_name, src_rel) in entries {
        let src = Path::new(&manifest).join(src_rel);
        println!("cargo:rerun-if-changed={}", src.display());
        tar.append_path_with_name(&src, archive_name)
            .unwrap_or_else(|e| panic!("bundling {src_rel}: {e}"));
    }

    tar.finish().expect("finalize tar");
}
