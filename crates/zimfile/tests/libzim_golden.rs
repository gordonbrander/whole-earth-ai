//! zimfile gives libzim's answers on zim-testing-suite. The answers were
//! recorded from libzim 9.8.2 (`tests/golden/libzim/`, made by zim-mcp's
//! former parity test): counts, main page, metadata, title-prefix ranges,
//! and every entry's path, title, mimetype, redirect target and content
//! SHA-256 (as one digest, plus a readable sample).

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use zimfile::Archive;

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

fn entry_line(a: &Archive, i: u32) -> String {
    let e = a.entry_at(i).unwrap();
    let target = e
        .redirect
        .map(|t| a.entry_at(t).unwrap().path)
        .unwrap_or_default();
    let sha = hex(&Sha256::digest(a.content(&e).unwrap()));
    format!(
        "{i}\t{}\t{}\t{}\t{target}\t{sha}\n",
        e.path, e.title, e.mimetype
    )
}

#[test]
fn matches_libzim() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let golden = root.join("golden/libzim");
    let mut n = 0;
    for f in std::fs::read_dir(&golden).unwrap() {
        let f = f.unwrap().path();
        let rel = f.file_name().unwrap().to_string_lossy().replace("__", "/");
        let zim = root.join("data").join(rel.strip_suffix(".tsv").unwrap());
        assert!(zim.exists(), "test data missing: run `just testdata`");
        let a = Archive::open(&zim).unwrap();
        let name = zim.display();
        for line in std::fs::read_to_string(&f).unwrap().lines() {
            let (key, rest) = line.split_once('\t').unwrap();
            match key {
                "all_entry_count" => assert_eq!(a.all_entry_count().to_string(), rest, "{name}"),
                "entry_count" => assert_eq!(a.entry_count().to_string(), rest, "{name}"),
                "article_count" => assert_eq!(a.article_count().to_string(), rest, "{name}"),
                "main" => {
                    let m = a.main_entry().map_or("-".into(), |e| e.path);
                    assert_eq!(m, rest, "{name}");
                }
                "meta" => {
                    let (m, want) = rest.split_once('\t').unwrap();
                    let got = a.metadata(m).unwrap_or_default();
                    let got = String::from_utf8_lossy(&got).escape_default().to_string();
                    assert_eq!(got, want, "{name}: {m}");
                }
                "suggest" => {
                    let (q, want) = rest.split_once('\t').unwrap();
                    let got: Vec<_> = a.suggest(q, 10).into_iter().map(|e| e.path).collect();
                    assert_eq!(got.join("|"), want, "{name}: {q:?}");
                }
                "entry" => {
                    let i: u32 = rest.split('\t').next().unwrap().parse().unwrap();
                    assert_eq!(entry_line(&a, i), format!("{rest}\n"), "{name}");
                }
                "entries_sha256" => {
                    let all: String = (0..a.all_entry_count())
                        .map(|i| entry_line(&a, i))
                        .collect();
                    assert_eq!(
                        hex(&Sha256::digest(all.as_bytes())),
                        rest,
                        "{name}: entries"
                    );
                }
                _ => panic!("{name}: unknown key {key}"),
            }
        }
        n += 1;
    }
    assert_eq!(n, 9);
}
