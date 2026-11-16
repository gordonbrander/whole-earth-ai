//! The tantivy sidecar: build, query, staleness, interrupted builds.

use std::path::{Path, PathBuf};
use zimfile::{Archive, search};

fn data() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    assert!(
        d.join("nons/small.zim").exists(),
        "test data missing: run `just testdata`"
    );
    d
}

/// A copy of `src` in a fresh temp dir, so the sidecar lands there.
fn scratch(src: &Path, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zimfile-search-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let zim = dir.join(src.file_name().unwrap());
    std::fs::copy(src, &zim).unwrap();
    zim
}

#[test]
fn build_and_query() {
    for cat in ["nons", "withns"] {
        let zim = scratch(
            &data()
                .join(cat)
                .join("wikipedia_en_climate_change_mini_2024-06.zim"),
            cat,
        );
        let a = Archive::open(&zim).unwrap();
        assert!(search::open(&a, &zim).is_none());
        let mut calls = 0;
        search::build(&a, &zim, |_, _| calls += 1).unwrap();
        assert!(calls > 0);
        assert!(search::sidecar_path(&zim).is_dir());
        assert!(!search::partial_path(&zim).exists());
        let idx = search::open(&a, &zim).unwrap();
        for (q, want) in [
            ("greenhouse gas", "Greenhouse_gas"),
            ("Kyoto Protocol", "Kyoto_Protocol"),
            ("sea level rise", "Sea_level_rise"),
        ] {
            let hits = idx.search(&a, q, 5).unwrap();
            assert!(
                hits.iter().any(|h| h.path.ends_with(want)),
                "{cat} {q:?}: {:?}",
                hits.iter().map(|h| &h.path).collect::<Vec<_>>()
            );
            assert!(
                hits.iter()
                    .all(|h| !h.snippet.is_empty() && !h.title.is_empty())
            );
            assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
        }
        // Every word must match (AND); stemming finds plurals.
        assert!(idx.search(&a, "greenhouse zzzqqq", 5).unwrap().is_empty());
        assert!(!idx.search(&a, "glaciers", 5).unwrap().is_empty());
        // Odd query syntax doesn't error.
        idx.search(&a, "\"unbalanced AND (", 5).unwrap();
        std::fs::remove_dir_all(zim.parent().unwrap()).unwrap();
    }
}

#[test]
fn stale_and_interrupted() {
    let zim = scratch(&data().join("nons/small.zim"), "stale");
    let a = Archive::open(&zim).unwrap();
    // An interrupted build leaves only `.part`: not an index, and rebuilt over.
    std::fs::create_dir_all(search::partial_path(&zim)).unwrap();
    std::fs::write(search::partial_path(&zim).join("junk"), "x").unwrap();
    assert!(search::open(&a, &zim).is_none());
    search::build(&a, &zim, |_, _| {}).unwrap();
    assert!(!search::partial_path(&zim).exists());
    let idx = search::open(&a, &zim).unwrap();
    assert!(!idx.search(&a, "test", 5).unwrap().is_empty() || a.article_count() <= 1);
    // An index of another archive (other uuid) is ignored.
    let stamp = search::sidecar_path(&zim).join("zimfile.json");
    let other = std::fs::read_to_string(&stamp).unwrap().replace('"', "'");
    std::fs::write(&stamp, other).unwrap();
    assert!(search::open(&a, &zim).is_none());
    std::fs::remove_dir_all(zim.parent().unwrap()).unwrap();
}

/// A built index opens on a read-only drive (no lock files are created).
#[cfg(unix)]
#[test]
fn read_only_index() {
    use std::os::unix::fs::PermissionsExt;
    let zim = scratch(&data().join("nons/small.zim"), "readonly");
    let a = Archive::open(&zim).unwrap();
    search::build(&a, &zim, |_, _| {}).unwrap();
    let set = |mode| {
        let idx = search::sidecar_path(&zim);
        for e in std::fs::read_dir(&idx).unwrap() {
            let p = e.unwrap().path();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode & 0o666)).unwrap();
        }
        for d in [idx.as_path(), zim.parent().unwrap()] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    };
    for lock in [".tantivy-meta.lock", ".tantivy-writer.lock"] {
        let _ = std::fs::remove_file(search::sidecar_path(&zim).join(lock));
    }
    set(0o555);
    let opened = search::open(&a, &zim).map(|i| i.search(&a, "test", 5).is_ok());
    set(0o755);
    assert_eq!(opened, Some(true));
    std::fs::remove_dir_all(zim.parent().unwrap()).unwrap();
}
