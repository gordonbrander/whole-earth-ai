//! Open every archive in openZIM's zim-testing-suite (`just testdata`): valid
//! ones read cleanly, `invalid.*` ones fail with `Err` somewhere — never a panic.

use std::path::{Path, PathBuf};
use zimfile::Archive;

pub fn data() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    assert!(
        d.join("withns/small.zim").exists(),
        "test data missing: run `just testdata`"
    );
    d
}

fn zims(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "zim"))
        .collect();
    v.sort();
    v
}

/// Read everything an archive offers; any inconsistency is an error.
fn walk(path: &Path) -> Result<(), String> {
    let a = Archive::open(path).map_err(|e| e.to_string())?;
    for i in 0..a.all_entry_count() {
        let e = a.entry_at(i).map_err(|e| e.to_string())?;
        a.content(&e).map_err(|err| format!("{}: {err}", e.path))?;
    }
    // Path lookups find the same entry (catches unsorted dirent tables).
    for i in 0..a.entry_count() {
        let e = a.entry_at(i).map_err(|e| e.to_string())?;
        match a.entry_by_path(&e.path) {
            Some(f) if f.index == i => {}
            _ => return Err(format!("lookup of {} failed", e.path)),
        }
    }
    let titles = a.front_articles().map_err(|e| e.to_string())?;
    let mut last = String::new();
    for t in titles {
        let e = a.entry_at(t).map_err(|e| e.to_string())?;
        if a.has_front_articles_index() && e.title < last {
            return Err(format!("title index unsorted at {}", e.title));
        }
        last = e.title;
    }
    if !a.verify_checksum().map_err(|e| e.to_string())? {
        return Err("checksum mismatch".into());
    }
    Ok(())
}

#[test]
fn testing_suite() {
    let d = data();
    for cat in ["withns", "nons", "noTitleListingV0"] {
        for z in zims(&d.join(cat)) {
            let name = z.file_name().unwrap().to_string_lossy().into_owned();
            let r = walk(&z);
            if name.starts_with("invalid.") {
                let e = r.expect_err(&format!("{cat}/{name} should fail"));
                eprintln!("{cat}/{name}: {e}");
            } else {
                r.unwrap_or_else(|e| panic!("{cat}/{name}: {e}"));
            }
        }
    }
}

#[test]
fn small_zim() {
    // "nons": new namespace scheme (no namespaces in user paths).
    let a = Archive::open(&data().join("nons/small.zim")).unwrap();
    assert_eq!(a.entry_count(), 2);
    assert_eq!(a.article_count(), 1);
    let main = a.main_entry().unwrap();
    assert!(main.is_redirect());
    let target = a.resolve(&main).unwrap();
    assert_eq!(target.path, "main.html");
    assert_eq!(target.mimetype, "text/html");
    assert_eq!(a.metadata("Title").unwrap(), b"Test ZIM file");
    assert!(a.metadata("Nope").is_none());
    assert!(a.entry_by_path("main.html").is_some());
    assert!(a.entry_by_path("A/main.html").is_some()); // long path, namespace stripped
    assert!(a.entry_by_path("nope.html").is_none());
    assert!(a.verify_checksum().unwrap());
    // "withns": old scheme, paths carry their namespace.
    let old = Archive::open(&data().join("withns/small.zim")).unwrap();
    assert_eq!((old.entry_count(), old.all_entry_count()), (17, 17));
    assert_eq!(old.entry_by_path("main.html").unwrap().path, "A/main.html");
    assert_eq!(
        old.entry_by_path("A/main.html").unwrap().path,
        "A/main.html"
    );
    assert_eq!(old.main_entry().unwrap().path, "A/main.html");
}

#[test]
fn split_archive() {
    for cat in ["withns", "nons"] {
        let d = data().join(cat);
        let whole = Archive::open(&d.join("wikibooks_be_all_nopic_2017-02.zim")).unwrap();
        for name in [
            "wikibooks_be_all_nopic_2017-02_splitted.zim",
            "wikibooks_be_all_nopic_2017-02_splitted.zimaa",
        ] {
            let split = Archive::open(&d.join(name)).unwrap();
            assert!(split.verify_checksum().unwrap());
            assert_eq!(split.all_entry_count(), whole.all_entry_count());
            for i in (0..whole.all_entry_count()).step_by(7) {
                let (a, b) = (whole.entry_at(i).unwrap(), split.entry_at(i).unwrap());
                assert_eq!(a, b);
                if !a.is_redirect() {
                    assert_eq!(whole.content(&a).unwrap(), split.content(&b).unwrap());
                }
            }
        }
    }
}

#[test]
fn missing_file() {
    assert!(Archive::open(&data().join("nope.zim")).is_err());
}

/// libzim `test/find.cpp`: `findByTitle` is a case-sensitive byte-prefix range.
#[test]
fn suggestions_by_title_prefix() {
    for cat in ["withns", "nons", "noTitleListingV0"] {
        let a =
            Archive::open(&data().join(cat).join("wikibooks_be_all_nopic_2017-02.zim")).unwrap();
        assert!(a.suggest("unkownTitle", 10).is_empty());
        assert!(a.suggest("j/body.js", 10).is_empty());
        assert!(a.suggest("", 10).is_empty());
        let first = a.suggest("Першая старонка", 10);
        // Old scheme: the page and its index.html redirect; new: `W/mainPage` isn't in `C`.
        assert_eq!(first.len(), if cat == "withns" { 2 } else { 1 }, "{cat}");
        let u = a.suggest("Украінская", 10);
        assert_eq!(u.len(), 5, "{cat}");
        assert!(u.iter().all(|e| e.title.starts_with("Украінская")));
        assert_eq!(a.suggest("Украінская", 2), u[..2]);
        assert!(a.suggest("украінская", 10).is_empty(), "case-sensitive");
    }
}

#[test]
fn not_found_by_path() {
    for cat in ["withns", "nons", "noTitleListingV0"] {
        let a =
            Archive::open(&data().join(cat).join("wikibooks_be_all_nopic_2017-02.zim")).unwrap();
        for p in ["unkwonPath", "U/unkwonPath", "A/unkwonPath", "X", "X/"] {
            assert!(a.entry_by_path(p).is_none(), "{cat} {p}");
        }
    }
}

/// `(path, category)` for `name` in every test-data category that has it.
fn data_files(name: &str) -> Vec<(PathBuf, &'static str)> {
    ["withns", "nons", "noTitleListingV0"]
        .into_iter()
        .map(|cat| (data().join(cat).join(name), cat))
        .filter(|(p, _)| p.exists() || p.with_extension("zimaa").exists())
        .collect()
}

// libzim test/archive.cpp:656 articleNumber
#[test]
fn article_number() {
    // name, media, then (articles, user entries, all entries) for withns, nons, noTitleListingV0.
    type Counts = (u32, u32, u32);
    let table: [(&str, u32, Counts, Counts, Counts); 4] = [
        ("small.zim", 1, (1, 17, 17), (1, 2, 16), (1, 2, 16)),
        (
            "wikibooks_be_all_nopic_2017-02.zim",
            34,
            (66, 118, 118),
            (66, 109, 123),
            (66, 109, 123),
        ),
        (
            "wikibooks_be_all_nopic_2017-02_splitted.zim",
            34,
            (66, 118, 118),
            (66, 109, 123),
            (66, 109, 123),
        ),
        (
            "wikipedia_en_climate_change_mini_2024-06.zim",
            111,
            (3821, 20565, 20565),
            (3821, 20551, 20568),
            (3821, 20551, 20567),
        ),
    ];
    let mut n = 0;
    for (name, media, withns, nons, v0) in table {
        for (p, cat) in data_files(name) {
            let want = match cat {
                "withns" => withns,
                "nons" => nons,
                _ => v0,
            };
            let a = Archive::open(&p).unwrap();
            let got = (a.article_count(), a.entry_count(), a.all_entry_count());
            assert_eq!(got, want, "{cat}/{name}");
            assert_eq!(a.media_count(), media, "{cat}/{name}");
            n += 1;
        }
    }
    assert_eq!(n, 12);
}

// libzim test/iterator.cpp:58 getEntry.indexOutOfRange
#[test]
fn entry_index_out_of_range() {
    let a = Archive::open(&data().join("withns/wikibooks_be_all_nopic_2017-02.zim")).unwrap();
    let e = a.entry_at(a.entry_count()).unwrap_err();
    assert_eq!(e.to_string(), "entry index out of range");
}

// libzim test/iterator.cpp:144 iteratorFunctions
#[test]
fn find_main_page_by_title() {
    for (p, cat) in data_files("wikipedia_en_climate_change_mini_2024-06.zim") {
        let a = Archive::open(&p).unwrap();
        let main = a.resolve(&a.main_entry().unwrap()).unwrap();
        let found = a.suggest(&main.title, 10);
        assert_eq!(found.len(), 1, "{cat}");
        assert_eq!(found[0].title, main.title);
    }
}

// libzim test/archive.cpp:542 openDontFallbackOnNonSplitZimArchive,
// :557 openNonExistantZimArchive, :571 openNonExistantZimSplitArchive
#[test]
fn open_errors() {
    for (p, _) in data_files("wikibooks_be_all_nopic_2017-02.zim") {
        let aa = PathBuf::from(format!("{}aa", p.display()));
        let e = Archive::open(&aa).err().unwrap();
        assert_eq!(
            e.to_string(),
            format!("Error opening as a split ZIM file: {}", aa.display())
        );
    }
    let e = Archive::open(Path::new("non_existant.zim")).err().unwrap();
    assert_eq!(e.to_string(), "Error opening ZIM file: non_existant.zim");
    let e = Archive::open(Path::new("non_existant.zimaa"))
        .err()
        .unwrap();
    assert_eq!(
        e.to_string(),
        "Error opening as a split ZIM file: non_existant.zimaa"
    );
}
