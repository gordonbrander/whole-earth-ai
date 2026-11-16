# zimfile

Read and write [ZIM](https://wiki.openzim.org/wiki/ZIM_file_format) archives
(Kiwix, openZIM) in pure Rust: a port of libzim 9.8.2's reader and writer,
including the Xapian full-text and title indexes Kiwix archives embed (read
and written, without Xapian), plus a [tantivy](https://github.com/quickwit-oss/tantivy)
index for archives that have none.

```rust
use zimfile::{Archive, Check, Content, Creator, Illustration, Item, search};

let a = Archive::open("wikipedia_en_100.zim".as_ref())?; // or a split .zimaa
let e = a.entry_by_path("Earth").unwrap();
let html = a.content(&e)?;                      // follows redirects
let head = a.read(&e, 0, 512)?;                 // a slice, without reading it all
let titles = a.suggest("ear", 10);              // the title index, as Kiwix ranks it
let hits = a.search("plate tectonics", 5)?;     // the full-text index (has_fulltext_index())
let text = zimfile::snippet(&String::from_utf8_lossy(&a.content(&hits[0].entry)?), "plate");
for t in a.find_by_title("Ear").skip(10).take(10) {
    println!("{}", a.entry_at_title(t)?.title);
}
for &i in a.cluster_order()? {                  // fastest order to read everything
    let _ = a.entry_at(i)?;
}
let icon = a.illustration(&Illustration::square(48));
a.check(Check::Checksum)?;                      // or zimfile::validate(path, &Check::ALL)

// Archives without an embedded index: build a tantivy one beside them.
search::build(&a, "wikipedia_en_100.zim".as_ref(), |_, _| {})?;
let idx = search::open(&a, "wikipedia_en_100.zim".as_ref()).unwrap();
let hits = idx.search(&a, "plate tectonics", 5)?;

let mut c = Creator::new("hello.zim".as_ref()).main_path("hello").index("eng");
c.add_metadata("Title", "Hello", "text/plain")?;
c.add_illustration(&Illustration::square(48), Content::File("icon.png".into()))?;
c.add_item(Item::new("hello", "Hello", "text/html", "<p>Hello</p>"))?;
c.add_item(Item::new("video.webm", "", "video/webm", Content::File("big.webm".into())))?;
c.add_alias("hi", "Hi", "hello", false)?;       // same content, no copy
c.finish()?;
```

Answers match libzim's on openZIM's test archives (path, title, mimetype,
redirects, content, metadata, counts, title suggestions, ranges, cluster
order, illustrations, integrity-check messages), and libzim reads what
`Creator` writes (`zimcheck -A` passes). libzim's own tests for these are
ported. `Content::File` items stream from disk when their cluster is
written: a 1 GiB file writes in under 3 MB of memory.

## Xapian indexes

`Archive::search`, `Archive::suggest` and `Creator::index` read and write the
Xapian indexes Kiwix archives embed, ranking as Kiwix does. The engine is its
own crate, [`zimfile-xapian`](../zimfile-xapian), which documents what's
supported, how it was checked against libzim, and its shortcuts.

Not ported from libzim:

- Cache knobs (cluster, dirent and lookup caches): fixed sizes.
- Opening from a file descriptor: `Archive::open_parts` takes paths and byte
  ranges, for embedded or arbitrarily split archives.
- The deprecated `getIllustrationSizes`, `configVerbose`, and the writer's
  worker threads (it compresses and indexes on one core).

Tantivy search indexes live in a hidden folder beside the archives, one per archive
(`.zimfile/index/<file>.zim.idx/`), so every program using zimfile shares them.
They can be deleted and rebuilt; `search::prune(dir)` removes the indexes of
archives that are gone.

License: GPL-3.0-or-later.
