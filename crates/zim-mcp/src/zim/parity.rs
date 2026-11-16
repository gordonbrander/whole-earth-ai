//! zimfile must answer exactly as libzim does (through the shim). Runs over
//! zim-testing-suite, the zim-mcp fixture, and `$ZIM_PARITY_FILE` if set
//! (large files are sampled).

use super::{Library, ffi};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn archives() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let suite = root.join("zimfile/tests/data");
    assert!(
        suite.join("nons/small.zim").exists(),
        "test data missing: run `just testdata`"
    );
    let mut out = vec![];
    for cat in ["withns", "nons", "noTitleListingV0"] {
        for e in std::fs::read_dir(suite.join(cat)).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if name.ends_with(".zim") && !name.starts_with("invalid.") {
                out.push(p);
            }
        }
    }
    for e in std::fs::read_dir(root.join("zim-mcp/tests/data")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "zim") {
            out.push(p);
        }
    }
    if let Some(f) = std::env::var_os("ZIM_PARITY_FILE") {
        out.push(f.into());
    }
    out.sort();
    out
}

/// One entry as libzim reports it: path, title, mimetype, redirect target path, content sha.
type Facts = (String, String, String, String, String);

fn libzim_entry(lib: &Library, i: u32) -> Option<Facts> {
    let e = ffi::entry_at(lib.lib(), 0, i).ok()?;
    let content = ffi::content_at(lib.lib(), 0, i).ok()?;
    Some((e.path, e.title, e.mimetype, e.redirect_path, sha(&content)))
}

fn zimfile_entry(a: &zimfile::Archive, i: u32) -> Option<Facts> {
    let e = a.entry_at(i).ok()?;
    let target = match e.redirect {
        Some(t) => a.entry_at(t).ok()?.path,
        None => String::new(),
    };
    let content = a.content(&e).ok()?;
    Some((e.path, e.title, e.mimetype, target, sha(&content)))
}

const METADATA: [&str; 9] = [
    "Title", "Language", "Date", "License", "Source", "Creator", "Scraper", "Counter", "Name",
];

const PREFIXES: [&str; 20] = [
    "A", "B", "C", "M", "N", "S", "T", "a", "m", "Л", "Б", "Укр", "Пер", "1", "The", "Nol", "Gen",
    "Africa", "main", "favicon",
];

fn check(path: &Path) {
    let name = path.display();
    let mut lib = Library::new();
    lib.open("x".into(), path).unwrap();
    let a = zimfile::Archive::open(path).unwrap();
    let all = ffi::all_entry_count(lib.lib(), 0).unwrap();
    assert_eq!(a.all_entry_count(), all, "{name}: all_entry_count");
    assert_eq!(a.entry_count(), lib.entry_count(0), "{name}: entry_count");
    assert_eq!(
        a.article_count(),
        lib.article_count(0),
        "{name}: article_count"
    );
    assert_eq!(
        a.main_entry().map(|e| e.path),
        ffi::main_path(lib.lib(), 0).ok(),
        "{name}: main entry"
    );
    for m in METADATA {
        let z = a
            .metadata(m)
            .map(|v| String::from_utf8_lossy(&v).into_owned());
        assert_eq!(
            z.unwrap_or_default(),
            lib.metadata(0, m),
            "{name}: metadata {m}"
        );
    }
    let step = (all / 100_000).max(1);
    for i in (0..all).step_by(step as usize) {
        assert_eq!(
            zimfile_entry(&a, i),
            libzim_entry(&lib, i),
            "{name}: entry {i}"
        );
    }
    // Path lookups, on a sample of user entries.
    for i in (0..lib.entry_count(0)).step_by(step as usize * 7) {
        let Ok(e) = ffi::entry_at(lib.lib(), 0, i) else {
            continue;
        };
        assert_eq!(
            a.entry_by_path(&e.path).map(|z| z.path),
            lib.entry(0, &e.path).map(|l| l.path),
            "{name}: lookup {}",
            e.path
        );
    }
    // libzim's suggester uses Xapian when the archive has a title index;
    // without one, it falls back to findByTitle, which zimfile ports.
    let xapian = ffi::has_title_index(lib.lib(), 0).unwrap();
    for q in PREFIXES {
        let z: Vec<_> = a.suggest(q, 10).into_iter().map(|e| e.path).collect();
        let range = ffi::title_range(lib.lib(), 0, q, 10).unwrap();
        assert_eq!(z, range, "{name}: findByTitle {q:?}");
        if !xapian {
            let l: Vec<_> = lib.suggest(0, q, 10).unwrap();
            let l: Vec<_> = l.into_iter().map(|h| h.path).collect();
            assert_eq!(z, l, "{name}: suggest {q:?}");
        }
    }
    eprintln!("parity ok: {name} ({all} entries, xapian titles: {xapian})");
}

#[test]
fn zimfile_matches_libzim() {
    for p in archives() {
        check(&p);
    }
}
