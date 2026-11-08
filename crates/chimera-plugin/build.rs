//! Embeds the website's build (`dist/`, from `bun run build`) as the editor's page. Without a build
//! the table is empty and the editor says so: `cargo test` needs nothing else to be built first.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    println!("cargo:rerun-if-changed=build.rs");
    let mut files = Vec::new();
    walk(&dist, &mut files);
    files.sort();
    let mut table = String::from("pub static ASSETS: &[(&str, &str, &[u8])] = &[\n");
    for f in &files {
        let rel = f.strip_prefix(&dist).unwrap().to_string_lossy().replace('\\', "/");
        // the plugin's Rust does the lyrics, so the page never fetches the dictionary
        if rel.ends_with(".map") || rel.starts_with("shows/") || rel == "data/cmudict.txt.gz" {
            continue;
        }
        let _ = writeln!(table, "    ({:?}, {:?}, include_bytes!({:?})),", format!("/{rel}"), mime(&rel), f.canonicalize().unwrap().display().to_string());
    }
    table.push_str("];\n");
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("assets.rs");
    std::fs::write(out, table).unwrap();
}
