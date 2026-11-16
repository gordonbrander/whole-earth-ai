# zimfile

Read and write [ZIM](https://wiki.openzim.org/wiki/ZIM_file_format) archives
(Kiwix, openZIM) in pure Rust: a port of libzim 9.8.2's reader and writer, plus
full-text search through a [tantivy](https://github.com/quickwit-oss/tantivy)
index per archive.

```rust
use zimfile::{Archive, Check, Content, Creator, Illustration, Item, search};

let a = Archive::open("wikipedia_en_100.zim".as_ref())?; // or a split .zimaa
let e = a.entry_by_path("Earth").unwrap();
let html = a.content(&e)?;                      // follows redirects
let head = a.read(&e, 0, 512)?;                 // a slice, without reading it all
let titles = a.suggest("Ear", 10);              // title prefix, as libzim
for t in a.find_by_title("Ear").skip(10).take(10) {
    println!("{}", a.entry_at_title(t)?.title);
}
for &i in a.cluster_order()? {                  // fastest order to read everything
    let _ = a.entry_at(i)?;
}
let icon = a.illustration(&Illustration::square(48));
a.check(Check::Checksum)?;                      // or zimfile::validate(path, &Check::ALL)

search::build(&a, "wikipedia_en_100.zim".as_ref(), |_, _| {})?;
let idx = search::open(&a, "wikipedia_en_100.zim".as_ref()).unwrap();
let hits = idx.search(&a, "plate tectonics", 5)?;

let mut c = Creator::new("hello.zim".as_ref()).main_path("hello");
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

Not ported from libzim:

- Xapian: Kiwix's embedded full-text and title indexes (`Searcher`,
  `SuggestionSearcher`, `hasFulltextIndex`, `hasTitleIndex`, the writer's
  indexing). zimfile's `search` is a tantivy index instead.
- Cache knobs (cluster, dirent and lookup caches): fixed sizes.
- Opening from a file descriptor: `Archive::open_parts` takes paths and byte
  ranges, for embedded or arbitrarily split archives.
- The deprecated `getIllustrationSizes`, `configVerbose`, and the writer's
  worker threads (it compresses on one core).

Search indexes live in a hidden folder beside the archives, one per archive
(`.zimfile/index/<file>.zim.idx/`), so every program using zimfile shares them.
They can be deleted and rebuilt; `search::prune(dir)` removes the indexes of
archives that are gone.

License: GPL-3.0-or-later.
