//! Write archives with `Creator`, read them back with `Archive`.

use std::path::{Path, PathBuf};
use zimfile::{Archive, Check, Compression, Content, Creator, Illustration, Item};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zimfile-writer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// A real 48×48 PNG, for zimcheck's illustration check.
const PNG: &[u8] = include_bytes!("../examples/dot.png");

fn html(body: &str) -> Vec<u8> {
    format!("<html><head><title>x</title></head><body><p>{body}</p></body></html>").into_bytes()
}

/// An archive exercising the writer's rules; returns its path.
fn sample(name: &str, compression: Compression, cluster_size: usize) -> PathBuf {
    let path = tmp(name);
    let mut c = Creator::new(&path)
        .compression(compression)
        .cluster_size(cluster_size)
        .main_path("Home")
        .uuid([7; 16]);
    c.add_metadata("Title", b"Test archive", "text/plain")
        .unwrap();
    c.add_metadata("Language", b"eng", "text/plain").unwrap();
    // The rest of zimcheck's mandatory metadata.
    for (k, v) in [
        ("Name", "test_en_sample"),
        ("Creator", "zimfile"),
        ("Publisher", "zimfile"),
        ("Date", "2026-11-16"),
        ("Description", "zimfile writer test"),
    ] {
        c.add_metadata(k, v.as_bytes(), "text/plain").unwrap();
    }
    c.add_illustration(&Illustration::square(48), PNG).unwrap();
    c.add_item(Item::new(
        "Home",
        "Home page",
        "text/html",
        html("Welcome home"),
    ))
    .unwrap();
    c.add_item(Item::new("Zebra", "", "text/html", html("Stripes")))
        .unwrap(); // empty title
    c.add_item(Item::new("Apple", "Apple", "text/html", html("Fruit")))
        .unwrap(); // title == path
    c.add_item(Item::new(
        "Ünïcødé/路径",
        "Ünïcødé",
        "text/html",
        html("Non-ASCII"),
    ))
    .unwrap();
    c.add_item(Item::new("style.css", "", "text/css", "p{}"))
        .unwrap();
    c.add_item(Item::new("img.png", "", "image/png", vec![0u8; 300]))
        .unwrap();
    c.add_item(Item::new("nomime", "", "", "x")).unwrap();
    let mut hidden = Item::new(
        "Hidden",
        "Hidden page",
        "text/html; charset=utf-8",
        html("Not front"),
    );
    hidden.front = Some(false);
    hidden.compress = Some(false);
    c.add_item(hidden).unwrap();
    for i in 0..40 {
        c.add_item(Item::new(
            &format!("Page{i:02}"),
            &format!("Page {i:02}"),
            "text/html",
            html(&"lorem ipsum ".repeat(i + 1)),
        ))
        .unwrap();
    }
    // Redirects: front and not, a chain, one added before its target.
    c.add_redirection("Fruit", "Fruit", "Apple", true).unwrap();
    c.add_redirection("Pomme", "Pomme", "Fruit", false).unwrap(); // chain → Apple
    c.add_redirection("Early", "Early", "Late", false).unwrap();
    c.add_item(Item::new(
        "Late",
        "Late",
        "text/html",
        html("Added after its redirect"),
    ))
    .unwrap();
    // Dropped: dangling, a loop.
    c.add_redirection("Dangling", "Dangling", "Missing", true)
        .unwrap();
    c.add_redirection("LoopA", "LoopA", "LoopB", false).unwrap();
    c.add_redirection("LoopB", "LoopB", "LoopA", false).unwrap();
    // Streamed from a file; an alias (another name for Home, not front).
    let notes = tmp(&format!("{name}.notes.txt"));
    std::fs::write(&notes, "Notes streamed from a file.").unwrap();
    c.add_item(Item::new(
        "notes.txt",
        "",
        "text/plain",
        Content::File(notes),
    ))
    .unwrap();
    c.add_alias("Home2", "Home again", "Home", false).unwrap();
    // Duplicates are refused.
    assert!(
        c.add_item(Item::new("Home", "Again", "text/html", "x"))
            .is_err()
    );
    assert!(c.add_redirection("Apple", "Apple", "Home", false).is_err());
    assert!(c.add_metadata("Title", b"Again", "text/plain").is_err());
    c.finish().unwrap();
    assert!(!tmp(&format!("{name}.tmp")).exists());
    path
}

fn check(path: &Path, many_clusters: bool) {
    let a = Archive::open(path).unwrap();
    for c in Check::ALL {
        a.check(c).unwrap_or_else(|e| panic!("{c:?}: {e}"));
    }
    assert_eq!(a.uuid(), [7; 16]);
    assert_eq!((a.header().major, a.header().minor), (6, 3));
    assert!(a.has_front_articles_index());
    assert_eq!(a.metadata("Title").unwrap(), b"Test archive");
    assert_eq!(a.metadata("Illustration_48x48@1").unwrap(), PNG);
    // Counter: C items by mimetype (parameters stripped), sorted.
    assert_eq!(
        String::from_utf8(a.metadata("Counter").unwrap()).unwrap(),
        "application/octet-stream=1;image/png=1;text/css=1;text/html=46;text/plain=1"
    );
    // Front articles: 45 HTML pages (not Hidden) + the Fruit redirect.
    assert_eq!(a.article_count(), 46);
    // C: 50 items + Fruit, Pomme, Early + the Home2 alias.
    assert_eq!(a.entry_count(), 54);
    assert_eq!(
        a.content(&a.entry_by_path("notes.txt").unwrap()).unwrap(),
        b"Notes streamed from a file."
    );
    let home2 = a.entry_by_path("Home2").unwrap();
    assert_eq!(
        (home2.title.as_str(), a.content(&home2).unwrap()),
        ("Home again", html("Welcome home"))
    );
    let main = a.main_entry().unwrap();
    assert_eq!(a.resolve(&main).unwrap().path, "Home");
    let home = a.entry_by_path("Home").unwrap();
    assert_eq!(
        (home.title.as_str(), home.mimetype.as_str()),
        ("Home page", "text/html")
    );
    assert_eq!(a.content(&home).unwrap(), html("Welcome home"));
    assert_eq!(a.entry_by_path("Zebra").unwrap().title, "Zebra");
    assert_eq!(
        a.entry_by_path("nomime").unwrap().mimetype,
        "application/octet-stream"
    );
    assert_eq!(
        a.entry_by_path("Hidden").unwrap().mimetype,
        "text/html; charset=utf-8"
    );
    let u = a.entry_by_path("Ünïcødé/路径").unwrap();
    assert_eq!(
        (u.title.as_str(), a.content(&u).unwrap()),
        ("Ünïcødé", html("Non-ASCII"))
    );
    assert_eq!(
        a.content(&a.entry_by_path("img.png").unwrap()).unwrap(),
        vec![0u8; 300]
    );
    let pomme = a.entry_by_path("Pomme").unwrap();
    assert_eq!(a.entry_at(pomme.redirect.unwrap()).unwrap().path, "Fruit");
    assert_eq!(a.content(&pomme).unwrap(), html("Fruit"));
    assert_eq!(
        a.resolve(&a.entry_by_path("Early").unwrap()).unwrap().path,
        "Late"
    );
    for gone in ["Dangling", "Missing", "LoopA", "LoopB"] {
        assert!(a.entry_by_path(gone).is_none(), "{gone}");
    }
    for i in 0..40 {
        let e = a.entry_by_path(&format!("Page{i:02}")).unwrap();
        assert_eq!(a.content(&e).unwrap(), html(&"lorem ipsum ".repeat(i + 1)));
    }
    // Title order, byte-wise.
    let titles: Vec<String> = a
        .front_articles()
        .unwrap()
        .into_iter()
        .map(|i| a.entry_at(i).unwrap().title)
        .collect();
    let mut sorted = titles.clone();
    sorted.sort();
    assert_eq!(titles, sorted);
    assert_eq!(a.suggest("Page 1", 100).len(), 10);
    assert!(a.header().cluster_count > if many_clusters { 5 } else { 1 });
}

#[test]
fn round_trip() {
    for (name, comp, size, many) in [
        ("zstd.zim", Compression::Zstd, 2 << 20, false),
        ("small_clusters.zim", Compression::Zstd, 200, true),
        ("none.zim", Compression::None, 1000, true),
        ("lzma.zim", Compression::Lzma, 2 << 20, false),
    ] {
        let p = sample(name, comp, size);
        check(&p, many);
        zimcheck(&p);
    }
}

/// `zimcheck -A` on our output, when `$ZIMCHECK` names the zimcheck binary.
/// Opt-in: an unsigned zimcheck on PATH can block on a macOS Gatekeeper prompt.
fn zimcheck(p: &Path) {
    let Some(bin) = std::env::var_os("ZIMCHECK") else {
        eprintln!("ZIMCHECK not set: skipping zimcheck -A");
        return;
    };
    let out = std::process::Command::new(bin)
        .arg("-A")
        .arg(p)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "zimcheck -A {}:\n{}",
        p.display(),
        String::from_utf8_lossy(&out.stdout)
    );
}

// libzim test/find.cpp:157 ByTitleWithDuplicate
#[test]
fn find_by_title_with_duplicates() {
    let path = tmp("dups.zim");
    let mut c = Creator::new(&path);
    for (i, t) in ["AAA", "BB", "BBB", "BBB", "BBBB", "CCC", "CCC"]
        .iter()
        .enumerate()
    {
        c.add_item(Item::new(&format!("article{i}"), t, "text/html", ""))
            .unwrap();
    }
    c.finish().unwrap();
    let a = Archive::open(&path).unwrap();
    for (q, n) in [("BBB", 3), ("BB", 4), ("BBBB", 1), ("CCC", 2), ("C", 2)] {
        let s = a.suggest(q, 100);
        assert_eq!(s.len(), n, "{q}");
        assert!(s.iter().all(|e| e.title.starts_with(q)));
    }
}

#[test]
fn empty_archive_and_unfinished() {
    let path = tmp("empty.zim");
    Creator::new(&path).main_path("nope").finish().unwrap();
    let a = Archive::open(&path).unwrap();
    assert_eq!((a.entry_count(), a.article_count()), (0, 0));
    assert!(a.main_entry().is_none());
    assert!(a.verify_checksum().unwrap());
    // An empty blob (zimcheck flags empty entries, so not in `sample`).
    let path = tmp("empty_blob.zim");
    let mut c = Creator::new(&path);
    c.add_item(Item::new("empty.txt", "", "text/plain", ""))
        .unwrap();
    c.finish().unwrap();
    let a = Archive::open(&path).unwrap();
    assert_eq!(
        a.content(&a.entry_by_path("empty.txt").unwrap()).unwrap(),
        b""
    );
    // Dropped without finish: no .tmp left behind.
    let path = tmp("unfinished.zim");
    let mut c = Creator::new(&path).cluster_size(10);
    for i in 0..5 {
        c.add_item(Item::new(&format!("p{i}"), "", "text/html", "some text"))
            .unwrap();
    }
    drop(c);
    assert!(!path.exists() && !tmp("unfinished.zim.tmp").exists());
}

// Mirror libzim test/creator.cpp:49 ASSERT_REDIRECT_ENTRY, :58 ASSERT_ITEM_ENTRY, :69 EXPECT_MISSING_ENTRY.
fn assert_item(a: &Archive, path: &str, title: &str, mime: &str, content: &str) {
    let e = a
        .entry_by_path(path)
        .unwrap_or_else(|| panic!("{path} missing"));
    assert!(!e.is_redirect(), "{path}");
    assert_eq!(
        (e.title.as_str(), e.mimetype.as_str()),
        (title, mime),
        "{path}"
    );
    assert_eq!(a.content(&e).unwrap(), content.as_bytes(), "{path}");
}

fn assert_redirect(a: &Archive, path: &str, title: &str, target: &str) {
    let e = a
        .entry_by_path(path)
        .unwrap_or_else(|| panic!("{path} missing"));
    assert_eq!(e.title, title, "{path}");
    let t = a.entry_at(e.redirect.expect("a redirect")).unwrap();
    assert_eq!(t.path, target, "{path}");
}

fn assert_missing(a: &Archive, path: &str) {
    assert!(a.entry_by_path(path).is_none(), "{path} should be missing");
}

/// libzim's `makeSafeUuid`: special characters in the uuid are not special.
fn safe_uuid() -> [u8; 16] {
    let mut u = [0x42; 16];
    u[5] = b'\n';
    u[10] = 0;
    u
}

/// A creator for test `name`, as libzim's tests set one up.
fn creator(name: &str) -> (Creator, PathBuf) {
    let p = tmp(name);
    (Creator::new(&p).uuid(safe_uuid()), p)
}

/// libzim's `TestItem`: HTML, a front article.
fn item(path: &str, title: &str, content: &str) -> Item {
    Item::new(path, title, "text/html", content)
}

// libzim test/creator.cpp:140 createEmptyZim
#[test]
fn create_empty_zim() {
    let (c, p) = creator("created_empty.zim");
    c.finish().unwrap();
    let a = Archive::open(&p).unwrap();
    assert!(a.main_entry().is_none());
    assert_eq!(a.all_entry_count(), 2); // counter + title listing
    let counter = a.entry_at(0).unwrap();
    assert_eq!(
        (counter.path.as_str(), counter.mimetype.as_str()),
        ("Counter", "text/plain")
    );
    let listing = a.entry_at(1).unwrap();
    assert_eq!(
        (listing.path.as_str(), listing.mimetype.as_str()),
        (
            "listing/titleOrdered/v1",
            "application/octet-stream+zimlisting"
        )
    );
    assert_eq!(a.content(&listing).unwrap(), b"");
    // Cluster 1 (the listing's) is uncompressed and holds one blob.
    let raw = std::fs::read(&p).unwrap();
    let u64_at = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().unwrap()) as usize;
    let c1 = u64_at(a.header().cluster_ptr_pos as usize + 8);
    assert_eq!(raw[c1], 1); // no compression
    assert_eq!(
        u32::from_le_bytes(raw[c1 + 1..c1 + 5].try_into().unwrap()),
        8
    ); // 1 blob
}

// libzim test/creator.cpp:360 titleListingProviderBatchBoundaries
#[test]
fn title_listing_batch_boundaries() {
    let per_batch = 64 * 1024 / 4;
    let first = 100000;
    for count in [
        0,
        per_batch - 1,
        per_batch,
        per_batch + 1,
        2 * per_batch,
        2 * per_batch + 1,
    ] {
        let (mut c, p) = creator("title_listing.zim");
        for i in 0..3 {
            let n = first - i - 1;
            let mut it = Item::new(
                &format!("path/{n}"),
                &format!("title/{n}"),
                "text/plain",
                "",
            );
            it.front = Some(false);
            c.add_item(it).unwrap();
        }
        for i in 0..count {
            // Title order is the reverse of path order.
            let title = format!("title/{}", first + count - i - 1);
            c.add_item(item(&format!("path/{}", first + i), &title, ""))
                .unwrap();
        }
        c.finish().unwrap();
        let a = Archive::open(&p).unwrap();
        let by_title = a.by_title();
        assert_eq!(by_title.len(), count, "{count}");
        for (i, t) in by_title.enumerate() {
            let e = a.entry_at_title(t).unwrap();
            assert_eq!(e.title, format!("title/{}", first + i));
            assert_eq!(e.path, format!("path/{}", first + count - i - 1));
        }
    }
}

// libzim test/creator.cpp:465 redirectAddedAfterItsTargetIsCreated
#[test]
fn redirect_added_after_its_target() {
    let (mut c, p) = creator("redirect_after.zim");
    c.add_item(item("item", "An item", "<html/>")).unwrap();
    c.add_redirection("redirect", "A redirect", "item", false)
        .unwrap();
    c.finish().unwrap();
    let a = Archive::open(&p).unwrap();
    assert_item(&a, "item", "An item", "text/html", "<html/>");
    assert_redirect(&a, "redirect", "A redirect", "item");
}

// libzim test/creator.cpp:563 attemptingToOverwriteAnItemWithAnotherItem,
// :584 …ARedirectWithAnotherRedirect, :612 …ARedirectWithAnItem, :640 …AnItemWithARedirect
#[test]
fn attempting_to_overwrite() {
    let (mut c, p) = creator("overwrite.zim");
    c.add_item(item("path", "An item", "I tempt you")).unwrap();
    assert!(
        c.add_item(item("path", "Another item", "It embodies me"))
            .is_err()
    );
    assert!(
        c.add_redirection("path", "A redirect", "anotherpath", false)
            .is_err()
    );
    c.add_item(item("itempath", "An item", "I tempt you"))
        .unwrap();
    c.add_redirection("redirectpath", "A redirect", "itempath", false)
        .unwrap();
    assert!(
        c.add_redirection("redirectpath", "Another redirect", "itempath", false)
            .is_err()
    );
    assert!(
        c.add_redirection("redirectpath", "A redirect", "anotherpath", false)
            .is_err()
    );
    c.add_item(item("path1", "Item#1", "I tempt you")).unwrap();
    c.add_redirection("path2", "A redirect", "path1", false)
        .unwrap();
    c.add_redirection("path3", "Second level redirect", "path2", false)
        .unwrap();
    assert!(
        c.add_item(item("path2", "Item#2", "I tempt you too"))
            .is_err()
    );
    c.finish().unwrap();
    let a = Archive::open(&p).unwrap();
    assert_item(&a, "path", "An item", "text/html", "I tempt you");
    assert_item(&a, "itempath", "An item", "text/html", "I tempt you");
    assert_redirect(&a, "redirectpath", "A redirect", "itempath");
    assert_item(&a, "path1", "Item#1", "text/html", "I tempt you");
    assert_redirect(&a, "path2", "A redirect", "path1");
    assert_redirect(&a, "path3", "Second level redirect", "path2");
    assert_missing(&a, "anotherpath");
}

// libzim test/creator.cpp:727 handlingOfAnAscendingBlindChainOfRedirections,
// :777 …ADescendingBlindChainOfRedirections
#[test]
fn blind_chains_of_redirections() {
    for chain in [
        ["redirectA", "redirectB", "redirectC"],
        ["redirectC", "redirectB", "redirectA"],
    ] {
        let (mut c, p) = creator("blind_chain.zim");
        c.add_item(item("1st", "1st entry in ZIM", "")).unwrap();
        c.add_redirection(chain[0], "First", chain[1], false)
            .unwrap();
        c.add_redirection(chain[1], "Middle", chain[2], false)
            .unwrap();
        c.add_redirection(chain[2], "Last", "missingTarget", false)
            .unwrap();
        c.finish().unwrap();
        let a = Archive::open(&p).unwrap();
        for gone in ["missingTarget", "redirectA", "redirectB", "redirectC"] {
            assert_missing(&a, gone);
        }
    }
}

// libzim test/creator.cpp:827 handlingOfRedirectionLoops
#[test]
fn redirection_loops() {
    let (mut c, p) = creator("loops.zim");
    for (from, to) in [
        ("A", "B"),
        ("B", "C"),
        ("C", "D"),
        ("D", "B"),
        ("S", "S"),
        ("Z", "Y"),
        ("Y", "X"),
        ("X", "W"),
        ("W", "Y"),
    ] {
        let (from, to) = (format!("redirect{from}"), format!("redirect{to}"));
        c.add_redirection(&from, &format!("{from} -> {to}"), &to, false)
            .unwrap();
    }
    c.finish().unwrap();
    let a = Archive::open(&p).unwrap();
    for x in "ABCDSWXYZ".chars() {
        assert_missing(&a, &format!("redirect{x}"));
    }
}

// libzim test/creator.cpp:884 pruningOfARedirectionForest
#[test]
fn pruning_of_a_redirection_forest() {
    let (mut c, p) = creator("forest.zim");
    c.add_item(item("root", "The only item", "ROOT")).unwrap();
    let edges = [
        // A tree leading into a loop.
        ("T", "N"),
        ("M", "N"),
        ("N", "L"),
        ("L", "0"),
        ("0", "O"),
        ("O", "P"),
        ("P", "L"),
        // A tree ending with a dangling redirect.
        ("A", "B"),
        ("B", "Q"),
        ("J", "K"),
        ("K", "Q"),
        ("Z", "Y"),
        ("Y", "Q"),
        ("Q", "(MISSING)"),
        // The only healthy tree.
        ("a", "b"),
        ("b", "q"),
        ("j", "k"),
        ("k", "q"),
        ("z", "y"),
        ("y", "q"),
        ("q", "root"),
    ];
    for (from, to) in edges {
        c.add_redirection(from, &format!("{from} -> {to}"), to, false)
            .unwrap();
    }
    c.finish().unwrap();
    let a = Archive::open(&p).unwrap();
    assert_missing(&a, "(MISSING)");
    assert_missing(&a, "0");
    for x in 'A'..='Z' {
        assert_missing(&a, &x.to_string());
    }
    assert_item(&a, "root", "The only item", "text/html", "ROOT");
    for (from, to) in &edges[14..] {
        assert_redirect(&a, from, &format!("{from} -> {to}"), to);
    }
}

/// libzim test/tools.cpp:182 createZimFromContent: `dummyPath<title>` items.
fn zim_from_content(name: &str, contents: &[(&str, &str)]) -> Archive {
    let (mut c, p) = creator(name);
    for (title, content) in contents {
        c.add_item(item(&format!("dummyPath{title}"), title, content))
            .unwrap();
    }
    c.add_metadata("Title", b"This is a title", "text/plain;charset=utf-8")
        .unwrap();
    c.finish().unwrap();
    Archive::open(&p).unwrap()
}

// libzim test/suggestion_iterator.cpp:127 rangeBased
#[test]
fn range_based_suggestions() {
    let a = zim_from_content(
        "range_based.zim",
        &[
            ("article a", "item a"),
            ("article b", "item b"),
            ("random c", "random c"),
        ],
    );
    let s = a.suggest("article", a.entry_count() as usize);
    assert_eq!(s.len(), 2);
    assert_eq!(
        (s[0].title.as_str(), s[0].path.as_str()),
        ("article a", "dummyPatharticle a")
    );
    assert_eq!(s[1].title, "article b");
}

// libzim test/archive.cpp:164 openCreatedArchive
#[test]
fn open_created_archive() {
    let (mut c, p) = creator("created.zim");
    c = c.main_path("foo");
    c.add_item(item("foo", "Foo", "FooContent")).unwrap();
    // Title order differs from path order.
    let mut foo2 = item("foo2", "AFoo", "Foo2Content");
    foo2.front = Some(false);
    c.add_item(foo2).unwrap();
    c.add_metadata("Title", b"This is a title", "text/plain;charset=utf-8")
        .unwrap();
    c.add_illustration(&Illustration::square(48), b"PNGBinaryContent48")
        .unwrap();
    c.add_illustration(&Illustration::square(96), b"PNGBinaryContent96")
        .unwrap();
    c.add_redirection("foo3", "FooRedirection", "foo", false)
        .unwrap();
    c.add_redirection("foo4", "FooRedirection", "NoExistent", false)
        .unwrap(); // dropped
    c.finish().unwrap();

    let a = Archive::open(&p).unwrap();
    // listing + Counter + Title + mainPage + 2 illustrations + 2 items + redirect
    assert_eq!(a.all_entry_count(), 9);
    assert_eq!(a.entry_count(), 3);
    assert_eq!(a.article_count(), 1);
    assert_eq!(a.uuid(), safe_uuid());
    assert_eq!(
        a.metadata_keys(),
        [
            "Counter",
            "Illustration_48x48@1",
            "Illustration_96x96@1",
            "Title"
        ]
    );
    assert_eq!(
        a.illustrations(),
        [Illustration::square(48), Illustration::square(96)]
    );
    assert_eq!(a.metadata("Title").unwrap(), b"This is a title");
    let title = a.metadata_entry("Title").unwrap();
    assert_eq!(title.mimetype, "text/plain;charset=utf-8");
    assert_eq!(a.entry_in('M', "Title").unwrap().index, title.index);
    assert_eq!(a.metadata("Counter").unwrap(), b"text/html=2");
    for size in [48, 96] {
        let e = a.illustration(&Illustration::square(size)).unwrap();
        assert_eq!(e.path, format!("Illustration_{size}x{size}@1"));
        assert_eq!(
            a.content(&e).unwrap(),
            format!("PNGBinaryContent{size}").as_bytes()
        );
        assert_eq!(a.metadata_entry(&e.path).unwrap().mimetype, "image/png");
    }
    let foo = a.entry_by_path("foo").unwrap();
    assert_eq!((foo.path.as_str(), foo.title.as_str()), ("foo", "Foo"));
    assert_eq!(a.content(&foo).unwrap(), b"FooContent");
    assert!(foo.redirect.is_none());
    assert_eq!(a.entry_in('C', "foo").unwrap().index, foo.index);
    assert_item(&a, "foo2", "AFoo", "text/html", "Foo2Content");
    let foo3 = a.entry_by_path("foo3").unwrap();
    assert_eq!(
        (foo3.title.as_str(), foo3.redirect),
        ("FooRedirection", Some(foo.index))
    );
    let main = a.main_entry().unwrap();
    assert_eq!(main.redirect, Some(foo.index));
    for missing in ["non/existent/path", "C/non/existent/path", "foo4"] {
        assert_missing(&a, missing);
    }
    assert!(a.entry_in('C', "non/existent/path").is_none());
}

/// Deterministic bytes that don't compress to nothing.
fn noise(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761) | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

#[test]
fn streams_files() {
    let jpeg = tmp("big.jpg");
    let big = noise(5 << 20, 1); // larger than a cluster
    std::fs::write(&jpeg, &big).unwrap();
    let txt = tmp("notes.txt");
    let text = "streamed text ".repeat(10_000);
    std::fs::write(&txt, &text).unwrap();
    for comp in [Compression::Zstd, Compression::Lzma, Compression::None] {
        let p = tmp(&format!("streams_{comp:?}.zim"));
        let mut c = Creator::new(&p).compression(comp).cluster_size(1 << 20);
        c.add_item(Item::new("a.html", "A", "text/html", "<p>before</p>"))
            .unwrap();
        c.add_item(Item::new(
            "big.jpg",
            "",
            "image/jpeg",
            Content::File(jpeg.clone()),
        ))
        .unwrap();
        c.add_item(Item::new(
            "notes.txt",
            "",
            "text/plain",
            Content::File(txt.clone()),
        ))
        .unwrap();
        c.add_item(Item::new("small.jpg", "", "image/jpeg", vec![9u8; 100]))
            .unwrap();
        c.add_metadata("Notes", Content::File(txt.clone()), "text/plain")
            .unwrap();
        c.add_item(Item::new("z.html", "Z", "text/html", "<p>after</p>"))
            .unwrap();
        c.finish().unwrap();
        let a = Archive::open(&p).unwrap();
        for ch in Check::ALL {
            a.check(ch)
                .unwrap_or_else(|e| panic!("{comp:?} {ch:?}: {e}"));
        }
        let get = |path| a.entry_by_path(path).unwrap();
        let e = get("big.jpg");
        assert_eq!(a.item_size(&e).unwrap(), big.len() as u64, "{comp:?}");
        assert_eq!(a.content(&e).unwrap(), big);
        assert_eq!(
            a.read(&e, 4 << 20, 10).unwrap(),
            big[4 << 20..(4 << 20) + 10]
        );
        assert!(
            a.direct_access(&e).is_some(),
            "images are stored uncompressed"
        );
        let e = get("notes.txt");
        assert_eq!(a.content(&e).unwrap(), text.as_bytes());
        assert_eq!(a.read(&e, 14, 8).unwrap(), b"streamed");
        assert_eq!(a.metadata("Notes").unwrap(), text.as_bytes());
        assert_eq!(a.content(&get("small.jpg")).unwrap(), vec![9u8; 100]);
        assert_eq!(a.content(&get("a.html")).unwrap(), b"<p>before</p>");
        assert_eq!(a.content(&get("z.html")).unwrap(), b"<p>after</p>");
    }
}

#[test]
fn file_changed_before_finish() {
    let f = tmp("changing.bin");
    for (name, change) in [("shrunk", 10usize), ("grew", 30)] {
        std::fs::write(&f, vec![1u8; 20]).unwrap();
        let p = tmp(&format!("changed_{name}.zim"));
        let mut c = Creator::new(&p);
        c.add_item(Item::new(
            "f.bin",
            "",
            "application/octet-stream",
            Content::File(f.clone()),
        ))
        .unwrap();
        std::fs::write(&f, vec![1u8; change]).unwrap();
        let e = c.finish().unwrap_err();
        assert!(
            e.to_string().contains("size changed while writing"),
            "{name}: {e}"
        );
        assert!(
            !p.exists() && !tmp(&format!("changed_{name}.zim.tmp")).exists(),
            "{name}"
        );
    }
    // A missing file is refused up front.
    let mut c = Creator::new(&tmp("missing.zim"));
    let missing = Item::new("m", "", "image/png", Content::File(tmp("no-such-file")));
    assert!(c.add_item(missing).is_err());
}

#[test]
fn failed_cluster_write_poisons_the_creator() {
    let f = tmp("poison.bin");
    std::fs::write(&f, vec![1u8; 20]).unwrap();
    let p = tmp("poisoned.zim");
    let mut c = Creator::new(&p).cluster_size(10);
    c.add_item(Item::new(
        "f.bin",
        "",
        "image/png",
        Content::File(f.clone()),
    ))
    .unwrap();
    std::fs::write(&f, b"x").unwrap();
    // The next uncompressed item closes the cluster holding f.bin: that fails…
    assert!(
        c.add_item(Item::new("g.png", "", "image/png", vec![2u8; 20]))
            .is_err()
    );
    // …and so does everything after.
    assert!(
        c.add_item(Item::new("h.png", "", "image/png", vec![3u8; 5]))
            .is_err()
    );
    assert!(c.finish().is_err());
    assert!(!p.exists() && !tmp("poisoned.zim.tmp").exists());
}

// libzim test/creator.cpp:526 aliases
#[test]
fn aliases() {
    let (mut c, p) = creator("aliases.zim");
    c.add_item(item("item", "An item", "<html/>")).unwrap();
    c.add_redirection("redirect", "A redirect", "item", false)
        .unwrap();
    c.add_alias("item_alias", "An item alias", "item", false)
        .unwrap();
    c.add_alias("redirect_alias", "A redirect alias", "redirect", false)
        .unwrap();
    let e = c.add_alias(
        "a_dangling_alias",
        "A dangling alias",
        "no_such_entry",
        false,
    );
    assert_eq!(
        e.unwrap_err().to_string(),
        "Impossible to alias C/no_such_entry as C/a_dangling_alias\nC/no_such_entry doesn't exist."
    );
    assert!(
        c.add_alias(
            "self_alias",
            "A self-referencing alias",
            "self_alias",
            false
        )
        .is_err()
    );
    c.finish().unwrap();
    let a = Archive::open(&p).unwrap();
    assert_item(&a, "item", "An item", "text/html", "<html/>");
    assert_item(&a, "item_alias", "An item alias", "text/html", "<html/>");
    assert_redirect(&a, "redirect", "A redirect", "item");
    assert_redirect(&a, "redirect_alias", "A redirect alias", "item");
    assert_missing(&a, "a_dangling_alias");
    assert_missing(&a, "self_alias");
    // Aliases aren't counted.
    assert_eq!(a.metadata("Counter").unwrap(), b"text/html=1");
}

// libzim test/creator.cpp:487 redirectAddedBeforeItsTargetIsCreated
#[test]
fn redirect_added_before_its_target() {
    let (mut c, p) = creator("redirect_before.zim");
    c.add_redirection("r1", "Redirect to an item", "item", false)
        .unwrap();
    c.add_redirection("r2", "Redirect to an item alias", "item_alias", false)
        .unwrap();
    c.add_redirection("r3", "Redirect to another redirect", "redirect", false)
        .unwrap();
    c.add_redirection("r4", "Redirect to a redirect alias", "redir_alias", false)
        .unwrap();
    c.add_item(item("item", "An item", "")).unwrap();
    c.add_alias("item_alias", "An alias of an item", "item", false)
        .unwrap();
    c.add_redirection("redirect", "Safe redirect to an item", "item", false)
        .unwrap();
    c.add_alias("redir_alias", "An alias of a redirect", "redirect", false)
        .unwrap();
    c.finish().unwrap();
    let a = Archive::open(&p).unwrap();
    assert_item(&a, "item", "An item", "text/html", "");
    assert_item(&a, "item_alias", "An alias of an item", "text/html", "");
    assert_redirect(&a, "redirect", "Safe redirect to an item", "item");
    assert_redirect(&a, "redir_alias", "An alias of a redirect", "item");
    assert_redirect(&a, "r1", "Redirect to an item", "item");
    assert_redirect(&a, "r2", "Redirect to an item alias", "item_alias");
    assert_redirect(&a, "r3", "Redirect to another redirect", "redirect");
    assert_redirect(&a, "r4", "Redirect to a redirect alias", "redir_alias");
}

// libzim test/creator.cpp:671 attemptingToOverwriteAnExistingEntryWithAnAlias
#[test]
fn attempting_to_overwrite_with_an_alias() {
    let (mut c, p) = creator("overwrite_alias.zim");
    c.add_item(item("item1", "Item#1", "It empowers everyone"))
        .unwrap();
    c.add_item(item("item2", "Item#2", "It embarrasses us"))
        .unwrap();
    c.add_redirection("redir1", "Redirect#1", "item1", false)
        .unwrap();
    c.add_redirection("redir2", "Redirect#2", "item2", false)
        .unwrap();
    for (path, target) in [
        ("item2", "item1"),
        ("item1", "item1"),
        ("item1", "redir1"),
        ("redir2", "redir1"),
        ("redir1", "redir1"),
        ("redir2", "item1"),
    ] {
        assert!(
            c.add_alias(path, "an alias", target, false).is_err(),
            "{path} -> {target}"
        );
    }
    c.finish().unwrap();
    let a = Archive::open(&p).unwrap();
    assert_item(&a, "item1", "Item#1", "text/html", "It empowers everyone");
    assert_item(&a, "item2", "Item#2", "text/html", "It embarrasses us");
    assert_redirect(&a, "redir1", "Redirect#1", "item1");
    assert_redirect(&a, "redir2", "Redirect#2", "item2");
}

// libzim test/creator.cpp:208 createZim, read at the format level as libzim does.
#[test]
fn create_zim() {
    use zimfile::format::{Cluster, Compression as C, Dirent, Header, Kind};
    let (mut c, p) = creator("create_zim.zim");
    c = c.main_path("foo");
    c.add_item(item("foo", "Foo", "FooContent")).unwrap();
    assert!(c.add_item(item("foo", "Foo", "FooContent")).is_err());
    c.add_item(item("foo2", "AFoo", "Foo2Content")).unwrap(); // title order ≠ path order
    c.add_alias("foo_bis", "The same Foo", "foo2", false)
        .unwrap();
    c.add_metadata("Title", "This is a title", "text/plain;charset=utf-8")
        .unwrap();
    c.add_illustration(&Illustration::square(48), "PNGBinaryContent48")
        .unwrap();
    c.add_illustration(&Illustration::square(96), "PNGBinaryContent96")
        .unwrap();
    c.add_redirection("foo3", "FooRedirection", "foo", false)
        .unwrap(); // not front
    c.add_alias("foo_ter", "The same redirection", "foo3", true)
        .unwrap(); // front
    c.add_redirection("foo4", "FooRedirection", "NoExistant", true)
        .unwrap(); // dropped
    c.finish().unwrap();

    let raw = std::fs::read(&p).unwrap();
    let h = Header::parse(&raw[..80]).unwrap();
    assert_ne!(h.main_page, u32::MAX);
    assert_eq!(h.entry_count, 11);
    assert_eq!(h.title_idx_pos, u64::MAX);
    let u64_at = |o: u64| u64::from_le_bytes(raw[o as usize..o as usize + 8].try_into().unwrap());
    let dirent = |i: u32| {
        let off = u64_at(h.path_ptr_pos + 8 * i as u64) as usize;
        Dirent::parse(&raw[off..]).unwrap().unwrap()
    };
    let (listing, png, html, plain, plain_utf8) = (0u16, 1, 2, 3, 4);
    let item_at = |i: u32, ns: u8, path: &str, title: &str, mime: u16, cluster: u32| {
        let d = dirent(i);
        assert_eq!(
            (d.ns, d.path.as_slice(), d.title.as_slice()),
            (ns, path.as_bytes(), title.as_bytes()),
            "#{i}"
        );
        let Kind::Item {
            mime: m,
            cluster: c,
            blob,
        } = d.kind
        else {
            panic!("#{i} is not an item")
        };
        assert_eq!((m, c), (mime, cluster), "#{i}");
        blob
    };
    let redirect_at = |i: u32, ns: u8, path: &str, title: &str, target: u32| {
        let d = dirent(i);
        assert_eq!(
            (d.ns, d.path.as_slice(), d.title()),
            (ns, path.as_bytes(), title.as_bytes()),
            "#{i}"
        );
        assert_eq!(d.kind, Kind::Redirect(target), "#{i}");
    };
    let foo = item_at(0, b'C', "foo", "Foo", html, 0);
    let foo2 = item_at(1, b'C', "foo2", "AFoo", html, 0);
    redirect_at(2, b'C', "foo3", "FooRedirection", 0);
    assert_eq!(item_at(3, b'C', "foo_bis", "The same Foo", html, 0), foo2);
    redirect_at(4, b'C', "foo_ter", "The same redirection", 0);
    let counter = item_at(5, b'M', "Counter", "", plain, 0);
    let ill48 = item_at(6, b'M', "Illustration_48x48@1", "", png, 1);
    let ill96 = item_at(7, b'M', "Illustration_96x96@1", "", png, 1);
    let title = item_at(8, b'M', "Title", "", plain_utf8, 0);
    redirect_at(9, b'W', "mainPage", "mainPage", 0);
    let v1 = item_at(10, b'X', "listing/titleOrdered/v1", "", listing, 1);

    let cluster = |i: u64| {
        Cluster::read(
            &raw[u64_at(h.cluster_ptr_pos + 8 * i) as usize..],
            h.entry_count as u64,
        )
        .unwrap()
    };
    let c0 = cluster(0);
    assert_eq!((c0.compression, c0.count()), (C::Zstd, 4));
    assert_eq!(c0.blob(foo), b"FooContent");
    assert_eq!(c0.blob(foo2), b"Foo2Content");
    assert_eq!(c0.blob(title), b"This is a title");
    assert_eq!(c0.blob(counter), b"text/html=2");
    let c1 = cluster(1);
    assert_eq!((c1.compression, c1.count()), (C::None, 3));
    // AFoo (1), Foo (0), The same redirection (4).
    assert_eq!(c1.blob(v1), [1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0]);
    assert_eq!(c1.blob(ill48), b"PNGBinaryContent48");
    assert_eq!(c1.blob(ill96), b"PNGBinaryContent96");
}
