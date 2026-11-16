//! Write archives with `Creator`, read them back with `Archive`.

use std::path::{Path, PathBuf};
use zimfile::{Archive, Compression, Creator, Item};

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
    c.add_illustration(48, PNG).unwrap();
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
    assert!(a.verify_checksum().unwrap());
    assert_eq!(a.uuid(), [7; 16]);
    assert_eq!((a.header().major, a.header().minor), (6, 3));
    assert!(a.has_front_articles_index());
    assert_eq!(a.metadata("Title").unwrap(), b"Test archive");
    assert_eq!(a.metadata("Illustration_48x48@1").unwrap(), PNG);
    // Counter: C items by mimetype (parameters stripped), sorted.
    assert_eq!(
        String::from_utf8(a.metadata("Counter").unwrap()).unwrap(),
        "application/octet-stream=1;image/png=1;text/css=1;text/html=46"
    );
    // Front articles: 45 HTML pages (not Hidden) + the Fruit redirect.
    assert_eq!(a.article_count(), 46);
    // C: 49 items + Fruit, Pomme, Early.
    assert_eq!(a.entry_count(), 52);
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

/// libzim `test/find.cpp` ByTitleWithDuplicate.
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

// libzim test/creator.cpp:49-70 ASSERT_ITEM_ENTRY, ASSERT_REDIRECT_ENTRY, EXPECT_MISSING_ENTRY.
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
