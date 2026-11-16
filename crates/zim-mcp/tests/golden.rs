//! Replays `list`, `read` and `export` on the fixture archive and compares
//! with outputs captured from the libzim build (`tests/golden/`), byte for byte.
//! Search checks only that the expected article is in the top 5.
//! `UPDATE_GOLDEN=1 cargo test --test golden` rewrites the files.

use std::path::{Path, PathBuf};
use std::process::Command;

const ARCHIVE: &str = "wikipedia_en_nollywood_maxi";

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn data_dir() -> PathBuf {
    let d = manifest().join("tests/data");
    let ok = std::fs::read_dir(&d).is_ok_and(|mut r| {
        r.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".zim")))
    });
    assert!(ok, "fixture archive missing: run `just testdata`");
    d
}

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_zim-mcp"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn check(name: &str, got: &str) {
    let file = manifest().join("tests/golden").join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(want == got, "{name} differs from golden output:\n{got}");
}

#[test]
fn list() {
    let d = data_dir();
    let d = d.to_str().unwrap();
    check("list.md", &run(&["list", "--dir", d]));
    check("list.json", &run(&["list", "--dir", d, "--json"]));
}

#[test]
fn read() {
    let d = data_dir();
    let d = d.to_str().unwrap();
    for (file, id, page) in [
        ("read_nollywood_1.md", "Nollywood", "1"),
        ("read_nollywood_3.md", "Nollywood", "3"),
        ("read_redirect.md", "10th_AMAA", "1"),
        ("read_genevieve.json", "Genevieve_Nnaji", "2"),
    ] {
        let id = format!("{ARCHIVE}/{id}");
        let mut args = vec!["read", &id, "--dir", d, "--page", page];
        if file.ends_with(".json") {
            args.push("--json");
        }
        check(file, &run(&args));
    }
}

fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(files(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}

#[test]
fn export() {
    let d = data_dir();
    let zim = files(&d)
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "zim"))
        .unwrap();
    let out = std::env::temp_dir().join(format!("zim-mcp-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    run(&[
        "export",
        zim.to_str().unwrap(),
        "Nollywood",
        "10th_AMAA",
        "Genevieve_Nnaji",
        "Kunle_Afolayan",
        "--out",
        out.to_str().unwrap(),
    ]);
    let got = files(&out);
    let golden = manifest().join("tests/golden/export");
    if std::env::var_os("UPDATE_GOLDEN").is_none() {
        let want: Vec<_> = files(&golden)
            .iter()
            .map(|p| p.strip_prefix(&golden).unwrap().to_path_buf())
            .collect();
        let have: Vec<_> = got
            .iter()
            .map(|p| p.strip_prefix(&out).unwrap().to_path_buf())
            .collect();
        assert_eq!(have, want);
    }
    for p in got {
        let rel = p.strip_prefix(&out).unwrap();
        check(
            &format!("export/{}", rel.display()),
            &std::fs::read_to_string(&p).unwrap(),
        );
    }
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn search() {
    let d = data_dir();
    let d = d.to_str().unwrap();
    let tsv = std::fs::read_to_string(manifest().join("tests/golden/search.tsv")).unwrap();
    for line in tsv.lines() {
        let (q, want) = line.split_once('\t').unwrap();
        let hits: serde_json::Value =
            serde_json::from_str(&run(&["search", q, "--dir", d, "--json"])).unwrap();
        let ids: Vec<&str> = hits
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&want), "{q:?}: {want} not in top 5 {ids:?}");
    }
}
