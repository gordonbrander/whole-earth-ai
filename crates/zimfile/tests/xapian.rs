//! zimfile's search and suggestions over archives' embedded Xapian indexes,
//! against libzim's results for the same queries (`tests/golden/xapian/`).

use std::path::PathBuf;
use zimfile::Archive;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `(archive, golden name)`: the climate archive (`just testdata`) and
/// zim-mcp's nollywood one.
fn archives() -> Vec<(Archive, &'static str)> {
    [
        (
            "tests/data/nons/wikipedia_en_climate_change_mini_2024-06.zim",
            "climate",
        ),
        (
            "../zim-mcp/tests/data/wikipedia_en_nollywood_maxi_2026-10.zim",
            "nollywood",
        ),
    ]
    .into_iter()
    .map(|(p, n)| {
        (
            Archive::open(&dir().join(p)).expect("test data missing: run `just testdata`"),
            n,
        )
    })
    .collect()
}

/// Golden results: (query, top-10 paths).
fn golden(file: &str) -> Vec<(String, Vec<String>)> {
    let text = std::fs::read_to_string(dir().join("tests/golden/xapian").join(file)).unwrap();
    let mut out: Vec<(String, Vec<String>)> = vec![];
    for line in text.lines() {
        if let Some(q) = line.strip_prefix("== ") {
            out.push((q.to_string(), vec![]));
        } else if !line.starts_with("# ") {
            out.last_mut().unwrap().1.push(line.to_string());
        }
    }
    out
}

/// How many queries give exactly libzim's top 10, in order (all, today).
fn compare(name: &str, run: impl Fn(&str) -> Vec<String>) -> usize {
    let mut exact = 0;
    let cases = golden(name);
    for (q, want) in &cases {
        let got = run(q);
        if &got == want {
            exact += 1;
        } else {
            eprintln!("{name}: {q:?}\n   ours {got:?}\n libzim {want:?}");
        }
    }
    eprintln!("{name}: {exact}/{} exact", cases.len());
    exact
}

#[test]
fn search_matches_libzim() {
    for (a, n) in archives() {
        assert!(a.has_fulltext_index());
        let exact = compare(&format!("{n}.search.txt"), |q| {
            a.search(q, 10)
                .unwrap()
                .into_iter()
                .map(|h| h.entry.path)
                .collect()
        });
        assert_eq!(exact, 30, "{n}: searches differing from libzim");
    }
}

#[test]
fn suggest_matches_libzim() {
    for (a, n) in archives() {
        assert!(a.has_title_index());
        let exact = compare(&format!("{n}.suggest.txt"), |q| {
            a.suggest(q, 10).into_iter().map(|e| e.path).collect()
        });
        assert_eq!(exact, 30, "{n}: suggestions differing from libzim");
    }
}
