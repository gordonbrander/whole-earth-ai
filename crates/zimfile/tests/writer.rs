//! Write archives with `Creator`, read them back with `Archive`.

use std::path::{Path, PathBuf};
use zimfile::{Archive, Compression, Creator, Item};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zimfile-writer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

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
    c.add_illustration(48, b"\x89PNG fake").unwrap();
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
    c.add_item(Item::new("empty.txt", "", "text/plain", ""))
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
    assert_eq!(a.metadata("Illustration_48x48@1").unwrap(), b"\x89PNG fake");
    // Counter: C items by mimetype (parameters stripped), sorted.
    assert_eq!(
        String::from_utf8(a.metadata("Counter").unwrap()).unwrap(),
        "application/octet-stream=1;image/png=1;text/css=1;text/html=46;text/plain=1"
    );
    // Front articles: 45 HTML pages (not Hidden) + the Fruit redirect.
    assert_eq!(a.article_count(), 46);
    // C: 50 items + Fruit, Pomme, Early.
    assert_eq!(a.entry_count(), 53);
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
        a.content(&a.entry_by_path("empty.txt").unwrap()).unwrap(),
        b""
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
