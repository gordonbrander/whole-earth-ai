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

fn wikibooks(cat: &str) -> Archive {
    Archive::open(&data().join(cat).join("wikibooks_be_all_nopic_2017-02.zim")).unwrap()
}

const CATEGORIES: [&str; 3] = ["withns", "nons", "noTitleListingV0"];

// libzim test/find.cpp:36 NotFoundByTitle, :48 NotFoundByPath
#[test]
fn not_found() {
    for cat in CATEGORIES {
        let a = wikibooks(cat);
        assert!(a.find_by_title("unkownTitle").is_empty());
        assert!(a.find_by_title("j/body.js").is_empty());
        for p in ["unkwonPath", "U/unkwonPath", "A/unkwonPath", "X", "X/"] {
            assert!(a.find_by_path(p).is_empty(), "{cat} {p}");
            assert!(a.entry_by_path(p).is_none(), "{cat} {p}");
        }
        assert!(a.find_by_title("").is_empty());
        assert!(a.suggest("", 10).is_empty());
    }
}

// libzim test/find.cpp:73 ByTitle
#[test]
fn find_by_title() {
    for cat in CATEGORIES {
        let a = wikibooks(cat);
        let titles = |r: &mut dyn Iterator<Item = u32>| -> Vec<String> {
            r.map(|t| a.entry_at_title(t).unwrap().title).collect()
        };
        let first = titles(&mut a.find_by_title("Першая старонка"));
        // Old scheme: the page and its index.html redirect; new: `W/mainPage` isn't in `C`.
        assert_eq!(first.len(), if cat == "withns" { 2 } else { 1 }, "{cat}");
        assert!(first.iter().all(|t| t.starts_with("Першая старонка")));
        let r = a.find_by_title("Украінская");
        let all = titles(&mut r.clone());
        assert_eq!(all.len(), 5, "{cat}");
        assert!(all.iter().all(|t| t.starts_with("Украінская")));
        // libzim's EntryRange::offset(start, max) is skip(start).take(max).
        assert_eq!(titles(&mut r.clone().take(2)), all[..2]);
        assert_eq!(titles(&mut r.clone().skip(1).take(4)), all[1..]);
        assert_eq!(titles(&mut r.clone().take(10)).len(), 5);
        assert!(titles(&mut r.clone().skip(10).take(5)).is_empty());
        assert_eq!(a.suggest("Украінская", 2), a.suggest("Украінская", 10)[..2]);
        assert!(a.find_by_title("украінская").is_empty(), "case-sensitive");
    }
}

// libzim test/find.cpp:183 ByPath
#[test]
fn find_by_path_withns() {
    let a = wikibooks("withns");
    for (prefix, first, count) in [
        ("A/Main_Page.html", 5, 1),
        ("I/s/", 78, 31),
        ("-/j/head.js", 2, 1),
        ("I", 75, 34),
        ("I/", 75, 34),
    ] {
        let r = a.find_by_path(prefix);
        assert_eq!((r.start, r.len()), (first, count), "{prefix}");
        for i in r {
            assert!(a.entry_at(i).unwrap().path.starts_with(prefix));
        }
    }
    assert_eq!(a.find_by_path(""), 0..118);
    assert_eq!(a.find_by_path("/"), 0..118);
}

// libzim test/find.cpp:256 ByPathNons
#[test]
fn find_by_path_nons() {
    for cat in ["nons", "noTitleListingV0"] {
        let a = wikibooks(cat);
        for (prefix, count) in [("Першая_старонка.html", 1), ("П", 2)] {
            let r = a.find_by_path(prefix);
            assert_eq!(r.len(), count, "{cat} {prefix}");
            for i in r {
                assert!(a.entry_at(i).unwrap().path.starts_with(prefix));
            }
        }
        assert_eq!(a.find_by_path(""), 0..109);
        assert_eq!(a.find_by_path("/"), 0..109);
    }
}

// libzim test/iterator.cpp:34 getEntryByClusterOrder, :77 IteratorTests.begin
#[test]
fn cluster_order() {
    let want: Vec<u32> = (0..75).chain(109..118).chain(75..109).collect();
    let a = wikibooks("withns");
    assert_eq!(a.cluster_order().unwrap(), want);
    for cat in ["nons", "noTitleListingV0"] {
        let a = wikibooks(cat);
        let mut got = a.cluster_order().unwrap().to_vec();
        got.sort();
        assert_eq!(got, a.by_path().collect::<Vec<_>>(), "{cat}: a permutation");
    }
}

// libzim test/iterator.cpp:100 beginByTitle, :124 beginByPath
#[test]
fn begin_by_title_and_path() {
    for cat in CATEGORIES {
        let a = wikibooks(cat);
        let want: Vec<u32> = if cat == "withns" {
            [5].into_iter().chain(7..16).collect()
        } else {
            (41..51).collect()
        };
        let got: Vec<u32> = a
            .by_title()
            .take(10)
            .map(|t| a.entry_at_title(t).unwrap().index)
            .collect();
        assert_eq!(got, want, "{cat}");
    }
    assert_eq!(
        wikibooks("withns").by_path().take(10).collect::<Vec<_>>(),
        (0..10).collect::<Vec<_>>()
    );
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
        let found = a.find_by_title(&main.title);
        assert_eq!(found.len(), 1, "{cat}");
        assert_eq!(a.entry_at_title(found.start).unwrap().title, main.title);
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

// libzim test/archive.cpp:289 openSplitZimArchive
#[test]
fn open_split_archive_path() {
    for (p, cat) in data_files("wikibooks_be_all_nopic_2017-02_splitted.zim") {
        let aa = PathBuf::from(format!("{}aa", p.display()));
        for path in [&p, &aa] {
            let a = Archive::open(path).unwrap();
            assert_eq!(a.path(), aa, "{cat}");
            assert!(a.is_multipart());
            assert!(a.verify_checksum().unwrap());
        }
    }
}

#[test]
fn checksum_and_size() {
    for (p, cat) in data_files("small.zim") {
        let a = Archive::open(&p).unwrap();
        let raw = std::fs::read(&p).unwrap();
        assert_eq!(a.file_size(), raw.len() as u64, "{cat}");
        assert_eq!(a.checksum().unwrap()[..], raw[raw.len() - 16..], "{cat}");
        assert!(!a.is_multipart());
        assert_eq!(a.path(), p);
    }
}

#[test]
fn metadata_keys_and_entries() {
    for (p, cat) in data_files("small.zim") {
        let a = Archive::open(&p).unwrap();
        let keys = a.metadata_keys();
        assert!(keys.contains(&"Title".to_string()), "{cat}: {keys:?}");
        assert!(keys.is_sorted());
        for k in &keys {
            let e = a.metadata_entry(k).unwrap();
            assert!(!e.is_redirect());
            assert_eq!(a.content(&e).unwrap(), a.metadata(k).unwrap());
            assert_eq!(a.entry_in('M', k).unwrap().namespace, 'M');
        }
        assert!(a.metadata_entry("Nope").is_none());
        assert!(
            a.metadata_entry("Title")
                .unwrap()
                .mimetype
                .starts_with("text/plain")
        );
    }
}

// libzim test/archive.cpp:584 randomEntry
#[test]
fn random_entry() {
    for name in [
        "wikibooks_be_all_nopic_2017-02.zim",
        "wikibooks_be_all_nopic_2017-02_splitted.zim",
        "wikipedia_en_climate_change_mini_2024-06.zim",
    ] {
        for (p, cat) in data_files(name) {
            let a = Archive::open(&p).unwrap();
            for _ in 0..20 {
                let e = a.resolve(&a.random_entry().unwrap()).unwrap();
                assert!(e.mimetype.contains("text/html"), "{cat}/{name}: {}", e.path);
            }
        }
    }
}

#[test]
fn entry_by_title_and_namespace() {
    for (p, cat) in data_files("wikibooks_be_all_nopic_2017-02.zim") {
        let a = Archive::open(&p).unwrap();
        let main = a.resolve(&a.main_entry().unwrap()).unwrap();
        let e = a.entry_by_title(&main.title).unwrap();
        assert_eq!(a.resolve(&e).unwrap().index, main.index, "{cat}");
        assert!(a.entry_by_title("unkownTitle").is_none());
        let ns = if cat == "withns" { 'A' } else { 'C' };
        assert_eq!(main.namespace, ns);
        let short = main.path.trim_start_matches("A/");
        assert_eq!(a.entry_in(ns, short).unwrap().index, main.index);
        assert!(a.entry_in('Q', short).is_none());
    }
}

/// `len` bytes at `off` in `path`, read as a direct-access caller would.
fn read_file_at(path: &Path, off: u64, len: u64) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).unwrap();
    f.seek(SeekFrom::Start(off)).unwrap();
    let mut v = vec![0; len as usize];
    f.read_exact(&mut v).unwrap();
    v
}

/// Every item with direct access reads the same from the file; returns how many.
fn check_direct_access(a: &Archive) -> usize {
    let mut n = 0;
    for i in 0..a.all_entry_count() {
        let e = a.entry_at(i).unwrap();
        if e.is_redirect() {
            continue;
        }
        if let Some((path, off)) = a.direct_access(&e) {
            let size = a.item_size(&e).unwrap();
            assert_eq!(
                read_file_at(&path, off, size),
                a.content(&e).unwrap(),
                "{}",
                e.path
            );
            n += 1;
        }
    }
    n
}

// libzim test/archive.cpp:1057 getDirectAccessInformation
#[test]
fn direct_access() {
    for (p, cat) in data_files("small.zim") {
        let a = Archive::open(&p).unwrap();
        assert_ne!(check_direct_access(&a), 0, "{cat}");
    }
}

#[test]
fn partial_reads() {
    for (p, cat) in data_files("wikipedia_en_climate_change_mini_2024-06.zim") {
        let a = Archive::open(&p).unwrap();
        // An HTML page (compressed) and the first image (stored).
        let html = a.resolve(&a.main_entry().unwrap()).unwrap();
        let img = (0..a.entry_count())
            .map(|i| a.entry_at(i).unwrap())
            .find(|e| e.mimetype.starts_with("image/"))
            .unwrap();
        for e in [html, img] {
            let all = a.content(&e).unwrap();
            let size = a.item_size(&e).unwrap();
            assert_eq!(size, all.len() as u64, "{cat} {}", e.path);
            assert_eq!(a.read(&e, 0, size).unwrap(), all);
            assert_eq!(a.read(&e, 3, 10).unwrap(), all[3..13]);
            assert_eq!(a.read(&e, size - 2, 100).unwrap(), all[all.len() - 2..]);
            assert!(a.read(&e, size, 10).unwrap().is_empty());
            assert!(a.read(&e, size + 1, 10).unwrap().is_empty());
        }
    }
}

// libzim test/archive.cpp:609 illustration
#[test]
fn illustration() {
    use zimfile::Illustration;
    for name in ["small.zim", "wikibooks_be_all_nopic_2017-02.zim"] {
        for (p, cat) in data_files(name) {
            let a = Archive::open(&p).unwrap();
            let e = a.illustration(&Illustration::square(48)).unwrap();
            let want = if cat == "withns" {
                "I/favicon.png"
            } else {
                "Illustration_48x48@1"
            };
            assert_eq!(e.path, want, "{cat}/{name}");
            assert_eq!(
                a.illustrations(),
                [Illustration::square(48)],
                "{cat}/{name}"
            );
            assert!(a.illustration(&Illustration::square(96)).is_none());
        }
    }
}

/// libzim test/archive.cpp:858 checkEquivalence.
fn check_equivalence(a: &Archive, b: &Archive) {
    assert_eq!(a.file_size(), b.file_size());
    assert_eq!(a.header().cluster_count, b.header().cluster_count);
    assert_eq!(a.entry_count(), b.entry_count());
    assert_ne!(a.entry_count(), 0);
    assert_eq!(a.main_entry().unwrap().title, b.main_entry().unwrap().title);
    let (oa, ob) = (a.cluster_order().unwrap(), b.cluster_order().unwrap());
    assert_eq!(oa, ob);
    for &i in oa {
        let (x, y) = (a.entry_at(i).unwrap(), b.entry_at(i).unwrap());
        assert_eq!(x, y);
        if !x.is_redirect() {
            assert_eq!(a.item_size(&x).unwrap(), b.item_size(&y).unwrap());
            assert_eq!(a.content(&x).unwrap(), b.content(&y).unwrap());
        }
    }
    assert_eq!(a.by_path(), b.by_path());
    let titles = |z: &Archive| -> Vec<u32> {
        z.by_title()
            .map(|t| z.entry_at_title(t).unwrap().index)
            .collect()
    };
    assert_eq!(titles(a), titles(b));
}

// libzim test/archive.cpp:937 multipart
#[test]
fn multipart() {
    for cat in CATEGORIES {
        let d = data().join(cat);
        let whole = Archive::open(&d.join("wikibooks_be_all_nopic_2017-02.zim")).unwrap();
        let split = Archive::open(&d.join("wikibooks_be_all_nopic_2017-02_splitted.zim")).unwrap();
        assert!(!whole.is_multipart());
        assert!(split.is_multipart());
        check_equivalence(&whole, &split);
    }
}

// libzim test/archive.cpp:977 openZIMFileEmbeddedInAnotherFile,
// :1099 getDirectAccessInformationFromEmbeddedArchive
#[test]
fn embedded_archive() {
    for (p, cat) in data_files("small.zim") {
        let normal = Archive::open(&p).unwrap();
        let host = PathBuf::from(format!("{}.embedded", p.display()));
        let embedded = Archive::open_parts(&[(host.clone(), 8, normal.file_size())]).unwrap();
        check_equivalence(&normal, &embedded);
        assert_eq!(embedded.path(), host, "{cat}");
        assert!(embedded.verify_checksum().unwrap());
        assert_ne!(check_direct_access(&embedded), 0, "{cat}");
    }
}

// libzim test/archive.cpp:993 openZIMFileMultiPartEmbeddedInAnotherFile
#[test]
fn multipart_embedded_archive() {
    for (p, cat) in data_files("small.zim") {
        let normal = Archive::open(&p).unwrap();
        let host = PathBuf::from(format!("{}.embedded.multi", p.display()));
        let mut parts = vec![];
        let mut left = normal.file_size();
        let mut start = "BEGINZIMMULTIPART".len() as u64;
        while left > 2048 {
            parts.push((host.clone(), start, 2048));
            start += 2048 + "NEWSECTIONZIMMULTI".len() as u64;
            left -= 2048;
        }
        parts.push((host.clone(), start, left));
        let multi = Archive::open_parts(&parts).unwrap();
        assert!(multi.is_multipart(), "{cat}");
        check_equivalence(&normal, &multi);
        assert!(multi.verify_checksum().unwrap());
        // Blobs across two parts have no direct access; the others read right.
        check_direct_access(&multi);
    }
}

// libzim test/archive.cpp:1024 checksumOfMultipartZim
#[test]
fn checksum_of_multipart_zim() {
    let p = data().join("noTitleListingV0/small.zim");
    let size = std::fs::metadata(&p).unwrap().len();
    let first = 1234; // not chunk-aligned
    let a =
        Archive::open_parts(&[(p.clone(), 0, first), (p.clone(), first, size - first)]).unwrap();
    assert!(a.is_multipart());
    assert!(a.verify_checksum().unwrap());
    // A range past the end of its file is refused.
    assert!(Archive::open_parts(&[(p.clone(), 1, size)]).is_err());
    assert!(Archive::open_parts(&[]).is_err());
}
