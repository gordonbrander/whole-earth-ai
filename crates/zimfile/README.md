# zimfile

Read and write [ZIM](https://wiki.openzim.org/wiki/ZIM_file_format) archives
(Kiwix, openZIM) in pure Rust: a port of libzim 9.8.2's reader and writer, plus
full-text search through a [tantivy](https://github.com/quickwit-oss/tantivy)
index per archive.

```rust
use zimfile::{Archive, Creator, Item, search};

let a = Archive::open("wikipedia_en_100.zim".as_ref())?;
let e = a.entry_by_path("Earth").unwrap();
let html = a.content(&e)?;                      // follows redirects
let titles = a.suggest("Ear", 10);              // title prefix, as libzim

search::build(&a, "wikipedia_en_100.zim".as_ref(), |_, _| {})?;
let idx = search::open(&a, "wikipedia_en_100.zim".as_ref()).unwrap();
let hits = idx.search(&a, "plate tectonics", 5)?;

let mut c = Creator::new("hello.zim".as_ref()).main_path("hello");
c.add_metadata("Title", b"Hello", "text/plain")?;
c.add_item(Item::new("hello", "Hello", "text/html", "<p>Hello</p>"))?;
c.finish()?;
```

Answers match libzim's on openZIM's test archives (path, title, mimetype,
redirects, content, metadata, counts, title suggestions), and libzim reads
what `Creator` writes. Kiwix's embedded Xapian indexes are not read.

Search indexes live in a hidden folder beside the archives, one per archive
(`.zimfile/index/<file>.zim.idx/`), so every program using zimfile shares them.
They can be deleted and rebuilt; `search::prune(dir)` removes the indexes of
archives that are gone.

License: GPL-3.0-or-later.
