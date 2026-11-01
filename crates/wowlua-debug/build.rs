//! wowlua's docs for `wowlua mcp`'s `wowlua_docs` (decision T551): every
//! `docs/**/*.md`, by its path under `docs/`, included in the binary.

use std::path::{Path, PathBuf};

fn main() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    println!("cargo:rerun-if-changed={}", docs.display());
    let mut files = Vec::new();
    walk(&docs, &mut files);
    files.sort();
    let mut out = String::from(
        "/// wowlua's docs: path under `docs/`, text.\npub static DOCS: &[(&str, &str)] = &[\n",
    );
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        let rel = f
            .strip_prefix(&docs)
            .expect("under docs")
            .to_string_lossy()
            .replace('\\', "/");
        let abs = f
            .canonicalize()
            .expect("a doc")
            .to_string_lossy()
            .into_owned();
        out.push_str(&format!("    ({rel:?}, include_str!({abs:?})),\n"));
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("docs.rs");
    std::fs::write(dest, out).expect("docs.rs written");
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            println!("cargo:rerun-if-changed={}", p.display());
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
}
