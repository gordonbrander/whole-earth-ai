//! libzim's Xapian suggestion and search tests (`test/suggestion.cpp`,
//! `test/search.cpp`, `test/search_iterator.cpp`), ported to archives
//! zimfile's `Creator::index` writes and `Archive::suggest`/`search` read.
//!
//! With `XAPIAN_EXPORT=<dir>`, each test copies its archive there and appends
//! its cases to `<dir>/cases.tsv`, for `tests/golden/xapian/check_libzim.py`
//! to check through real libzim.

use std::io::Write;
use std::path::PathBuf;
use zimfile::{Archive, Creator, Item, SearchHit};

struct Zim {
    name: String,
    path: PathBuf,
    a: Archive,
}

impl Drop for Zim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Build `<name>.zim` (indexed in `lang`, if any) with `add`, and open it.
fn build(
    name: &str,
    lang: Option<&str>,
    main: Option<&str>,
    add: impl FnOnce(&mut Creator) -> zimfile::Result<()>,
) -> Zim {
    let dir = std::env::temp_dir().join(format!("zimfile-libzim-xapian-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.zim"));
    let mut c = Creator::new(&path);
    if let Some(l) = lang {
        c = c.index(l);
    }
    if let Some(m) = main {
        c = c.main_path(m);
    }
    add(&mut c).unwrap();
    c.finish().unwrap();
    if let Ok(x) = std::env::var("XAPIAN_EXPORT") {
        std::fs::copy(&path, PathBuf::from(x).join(format!("{name}.zim"))).unwrap();
    }
    Zim {
        name: name.into(),
        a: Archive::open(&path).unwrap(),
        path,
    }
}

/// libzim's `TestItem`: content defaults to "foo".
fn item(path: &str, title: &str, content: &str) -> Item {
    Item::new(path, title, "text/html", content.as_bytes().to_vec())
}

const TITLE_MIME: &str = "text/plain;charset=utf-8";

/// `createZimFromTitles`.
fn from_titles(name: &str, titles: &[&str]) -> Zim {
    build(name, Some("en"), None, |c| {
        for t in titles {
            c.add_item(item(&format!("dummyPath/{t}"), t, "foo"))?;
        }
        c.add_metadata("Title", b"This is a title".to_vec(), TITLE_MIME)
    })
}

/// `createZimFromContent`: `(title, content)`s.
fn from_content(name: &str, contents: &[(&str, &str)]) -> Zim {
    build(name, Some("en"), None, |c| {
        for (t, body) in contents {
            c.add_item(item(&format!("dummyPath{t}"), t, body))?;
        }
        c.add_metadata("Title", b"This is a title".to_vec(), TITLE_MIME)
    })
}

/// `TempZimArchiveMadeOfEmptyHtmlArticles`: `(path, title)`s.
fn empty_articles(name: &str, lang: &str, data: &[(&str, &str)]) -> Zim {
    build(name, Some(lang), None, |c| {
        for (p, t) in data {
            c.add_item(item(p, t, "foo"))?;
        }
        c.add_metadata("Title", b"Test zim".to_vec(), TITLE_MIME)
    })
}

impl Zim {
    /// libzim's `getEntryCount`, the limit its tests use.
    fn n(&self) -> usize {
        self.a.entry_count() as usize
    }

    /// The paths of the entries titled `titles`; equal titles take the
    /// entries in path order (their docid order).
    fn paths_of(&self, titles: &[&str]) -> Vec<String> {
        let mut pool: Vec<_> = self
            .a
            .by_path()
            .map(|i| self.a.entry_at(i).unwrap())
            .collect();
        titles
            .iter()
            .map(|t| {
                let i = pool.iter().position(|e| e.title == *t);
                pool.remove(i.unwrap_or_else(|| panic!("no entry titled {t:?}")))
                    .path
            })
            .collect()
    }

    fn export(&self, kind: &str, q: &str, limit: usize, paths: &[String]) {
        let Ok(x) = std::env::var("XAPIAN_EXPORT") else {
            return;
        };
        let line = format!(
            "{}.zim\t{kind}\t{q}\t{limit}\t{}\n",
            self.name,
            paths.join("|")
        );
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(PathBuf::from(x).join("cases.tsv"))
            .unwrap();
        f.write_all(line.as_bytes()).unwrap();
    }

    /// Assert libzim's suggestions (titles, in order) for `q`.
    fn suggest(&self, q: &str, limit: usize, want: &[&str]) {
        self.export("suggest", q, limit, &self.paths_of(want));
        let got: Vec<_> = self
            .a
            .suggest(q, limit)
            .into_iter()
            .map(|e| e.title)
            .collect();
        assert_eq!(got, want, "suggest {q:?}");
    }

    /// `EXPECT_SUGGESTED_TITLES`: everything up to the entry count.
    fn suggest_all(&self, q: &str, want: &[&str]) {
        self.suggest(q, self.n(), want);
    }

    /// Search hits, exported as found: for checks that pin only part of the
    /// result, libzim then checks the rest of it.
    fn search(&self, q: &str, limit: usize) -> Vec<SearchHit> {
        let hits = self.a.search(q, limit).unwrap();
        let paths: Vec<_> = hits.iter().map(|h| h.entry.path.clone()).collect();
        self.export("search", q, limit, &paths);
        hits
    }

    /// Assert the whole search result (titles, in order).
    fn search_exact(&self, q: &str, limit: usize, want: &[&str]) -> Vec<SearchHit> {
        self.export("search", q, limit, &self.paths_of(want));
        let hits = self.a.search(q, limit).unwrap();
        let got: Vec<_> = hits.iter().map(|h| h.entry.title.as_str()).collect();
        assert_eq!(got, want, "search {q:?}");
        hits
    }
}

// ---- suggestion.cpp ----

// Suggestion.searchByTitle: skipped, needs WITH_TEST_DATA archives.

const BERLINS: &[&str] = &[
    "fooland",
    "berlin wall",
    "hotel berlin, berlin",
    "again berlin",
    "berlin",
    "not berlin",
];

#[test]
fn suggestion_empty_query() {
    let z = from_titles("suggestion_empty_query", BERLINS);
    z.suggest_all("", &[]);
}

#[test]
fn suggestion_no_result() {
    // libzim's list misses a comma: "foolandberlin wall" is one title.
    let z = from_titles(
        "suggestion_no_result",
        &[
            "foolandberlin wall",
            "hotel berlin, berlin",
            "again berlin",
            "berlin",
            "not berlin",
        ],
    );
    z.suggest_all("none", &[]);
}

#[test]
fn suggestion_single_term_order() {
    let z = from_titles("suggestion_single_term_order", BERLINS);
    z.suggest_all(
        "berlin",
        &[
            "berlin",
            "berlin wall",
            "hotel berlin, berlin",
            "again berlin",
            "not berlin",
        ],
    );
}

#[test]
fn suggestion_case_diacritics_and_homographs_handling() {
    let z = from_titles(
        "suggestion_case_diacritics_and_homographs_handling",
        &[
            "nonberlin",
            "simply berlin",
            "accented bérlin",
            "uppercase BERLIN",
            "homograph bеrlin", // е is cyrillic
        ],
    );
    let want = ["accented bérlin", "simply berlin", "uppercase BERLIN"];
    z.suggest_all("berlin", &want);
    z.suggest_all("BERLIN", &want);
    z.suggest_all("bêřlïñ", &want);
    // е in the query is cyrillic
    z.suggest_all("bеrlin", &["homograph bеrlin"]);
}

#[test]
fn suggestion_results_greater_than_limit() {
    let z = from_titles(
        "suggestion_results_greater_than_limit",
        &["foobar b", "foobar a", "foobar c", "foobar e", "foobar d"],
    );
    z.suggest("foobar", 2, &["foobar a", "foobar b"]);
}

const WOLVES: &[&str] = &[
    "The chocolate factory",
    "The wolf of Shingashina",
    "The wolf of Wall Street",
    "Hour of the wolf",
    "Wolf",
    "Terma termb the wolf of wall street termc",
];

#[test]
fn suggestion_partial_query() {
    let z = from_titles("suggestion_partial_query", WOLVES);
    z.suggest_all(
        "Wo",
        &[
            "Wolf",
            "Hour of the wolf",
            "The wolf of Shingashina",
            "The wolf of Wall Street",
            "Terma termb the wolf of wall street termc",
        ],
    );
}

#[test]
fn suggestion_phrase_order() {
    let z = from_titles(
        "suggestion_phrase_order",
        &[
            "summer winter autumn",
            "winter autumn summer terma",
            "autumn summer winter",
            "control document",
            "summer",
        ],
    );
    z.suggest_all(
        "winter autumn summer",
        &[
            "winter autumn summer terma",
            "autumn summer winter",
            "summer winter autumn",
        ],
    );
}

#[test]
fn suggestion_incremental_search() {
    let z = from_titles(
        "suggestion_incremental_search",
        &[
            "The chocolate factory",
            "The wolf of Shingashina",
            "The wolf of Wall Street",
            "The wolf among sheeps",
            "The wolf of Wall Street Book",
            "Hour of the wolf",
            "Wolf",
            "Terma termb the wolf of wall street termc",
            "Are there any beasts in this country?",
        ],
    );
    let wolf = [
        "Wolf",
        "Hour of the wolf",
        "The wolf among sheeps",
        "The wolf of Shingashina",
        "The wolf of Wall Street",
        "The wolf of Wall Street Book",
        "Terma termb the wolf of wall street termc",
    ];
    z.suggest_all("Wolf", &wolf);
    z.suggest_all("Wolf ", &wolf);
    let the = [
        "The chocolate factory",
        "The wolf among sheeps",
        "The wolf of Shingashina",
        "The wolf of Wall Street",
        "The wolf of Wall Street Book",
        "Hour of the wolf",
        "Terma termb the wolf of wall street termc",
    ];
    let mut the_partial = the.to_vec();
    the_partial.push("Are there any beasts in this country?");
    z.suggest_all("the", &the_partial);
    z.suggest_all("the ", &the);
    z.suggest_all(
        "the wol",
        &[
            "Hour of the wolf",
            "The wolf among sheeps",
            "The wolf of Shingashina",
            "The wolf of Wall Street",
            "The wolf of Wall Street Book",
            "Terma termb the wolf of wall street termc",
        ],
    );
    let the_wolf = [
        "The wolf among sheeps",
        "The wolf of Shingashina",
        "The wolf of Wall Street",
        "The wolf of Wall Street Book",
        "Hour of the wolf",
        "Terma termb the wolf of wall street termc",
    ];
    z.suggest_all("the wolf", &the_wolf);
    z.suggest_all("the wolf ", &the_wolf);
    z.suggest_all(
        "the wolf of",
        &[
            "The wolf of Shingashina",
            "The wolf of Wall Street",
            "The wolf of Wall Street Book",
            "Terma termb the wolf of wall street termc",
            "Hour of the wolf",
        ],
    );
    z.suggest_all(
        "the wolf of wall",
        &[
            "The wolf of Wall Street",
            "The wolf of Wall Street Book",
            "Terma termb the wolf of wall street termc",
        ],
    );
}

#[test]
fn suggestion_phrase_out_of_window() {
    let z = from_titles(
        "suggestion_phrase_out_of_window",
        &[
            "This query",
            "This is the dummy query phrase",
            "the aterm bterm dummy cterm query",
            "aterm the bterm dummy query cterm",
        ],
    );
    z.suggest_all(
        "the dummy query",
        &[
            "This is the dummy query phrase",
            "aterm the bterm dummy query cterm",
            "the aterm bterm dummy cterm query",
        ],
    );
}

// Suggestion.checkStopword: skipped, depends on libzim's English stopwords.

#[test]
fn suggestion_check_redirection_collapse() {
    let z = build(
        "suggestion_check_redirection_collapse",
        Some("en"),
        None,
        |c| {
            c.add_item(item("testPath", "Article Target", "foo"))?;
            c.add_redirection("redirectionPath1", "Article Redirect 1", "testPath", false)?;
            c.add_redirection("redirectionPath2", "Article Redirect 2", "testPath", false)?;
            c.add_metadata("Title", b"Test zim".to_vec(), TITLE_MIME)
        },
    );
    z.suggest_all("Article", &["Article Target"]);
}

#[test]
fn suggestion_check_redirection_chain() {
    let z = build(
        "suggestion_check_redirection_chain",
        Some("en"),
        None,
        |c| {
            c.add_item(item("testPath", "Article Target", "foo"))?;
            c.add_redirection("redirectionPath1", "Article Redirect 1", "testPath", false)?;
            c.add_redirection(
                "redirectionPath2",
                "Article Redirect 2",
                "redirectionPath1",
                true,
            )?;
            c.add_metadata("Title", b"Test zim".to_vec(), TITLE_MIME)
        },
    );
    z.suggest_all("Article", &["Article Target", "Article Redirect 2"]);
}

#[test]
fn suggestion_diff_article_same_title() {
    let z = build(
        "suggestion_diff_article_same_title",
        Some("en"),
        None,
        |c| {
            c.add_item(item("testPath1", "Test Article", "foo"))?;
            c.add_item(item("testPath2", "Test Article", "foo"))?;
            c.add_metadata("Title", b"Test zim".to_vec(), TITLE_MIME)
        },
    );
    z.suggest_all("Test Article", &["Test Article", "Test Article"]);
}

#[test]
fn suggestion_anchor_query_to_beginning() {
    let z = from_titles(
        "suggestion_anchor_query_to_beginning",
        &[
            "aterm bterm this is a title cterm",
            "this is a title aterm bterm cterm",
            "aterm this is a title bterm cterm",
        ],
    );
    z.suggest_all(
        "This is a title",
        &[
            "this is a title aterm bterm cterm",
            "aterm bterm this is a title cterm",
            "aterm this is a title bterm cterm",
        ],
    );
}

#[test]
fn suggestion_non_word_characters() {
    let z = from_titles(
        "suggestion_non_word_characters",
        &[
            "Alice Bob",
            "Alice & Bob",
            "Bonnie + Clyde",
            "Jack & Jill, on the hill",
            "Ali Baba & the 40 thieves",
            "&",
            "&%#",
        ],
    );
    z.suggest_all(
        "Ali",
        &["Ali Baba & the 40 thieves", "Alice & Bob", "Alice Bob"],
    );
    z.suggest_all("Alice Bob", &["Alice & Bob", "Alice Bob"]);
    z.suggest_all("Alice & Bob", &["Alice & Bob", "Alice Bob"]);
    z.suggest_all("Bonnie + Clyde", &["Bonnie + Clyde"]);
    z.suggest_all("Jack & Jill", &["Jack & Jill, on the hill"]);
    z.suggest_all("4", &["Ali Baba & the 40 thieves"]);
    z.suggest_all("40", &["Ali Baba & the 40 thieves"]);
    // In the other titles "&" is noise.
    z.suggest_all("&", &["&", "&%#"]);
    z.suggest_all("&%#", &["&%#"]);
}

#[test]
fn suggestion_titles_made_of_stop_words_only() {
    let z = from_titles(
        "suggestion_titles_made_of_stop_words_only",
        &[
            "The",
            "Are you at home?",
            "Back and forth",
            "One, two, three...",
            "Not at all",
            "Do not act before you have to",
        ],
    );
    z.suggest_all("the", &["The"]);
    z.suggest_all("not", &["Not at all", "Do not act before you have to"]);
    z.suggest_all("at", &["Not at all", "Are you at home?"]);
}

// Suggestion.titleSnippet: skipped, snippets.

#[test]
fn suggestion_reuse_searcher() {
    let z = from_titles(
        "suggestion_reuse_searcher",
        &[
            "song for you",
            "sing a song for you",
            "a song b for c you",
            "song for someone",
        ],
    );
    // getResults(0, 2), then getResults(2, n): 3 in all.
    assert_eq!(z.a.suggest("song for you", 2).len(), 2);
    let all = z.a.suggest("song for you", z.n());
    assert_eq!(all.len(), 3);
    let paths: Vec<_> = all.into_iter().map(|e| e.path).collect();
    z.export("suggest", "song for you", z.n(), &paths);
}

#[test]
fn suggestion_cjk() {
    let z = empty_articles(
        "suggestion_cjk",
        "zh",
        &[("testPath1", "平方"), ("testPath2", "平方根")],
    );
    z.suggest_all("平方", &["平方", "平方根"]);
    z.suggest_all("平方根", &["平方根"]);
}

/// libzim's `MAX_INDEXABLE_TITLE_WORD_SIZE`.
const MAX_WORD_LENGTH: usize = 240;

fn long_word(n: usize) -> String {
    let s = format!("awordthatis{n}characterslong");
    let last = s.chars().last().unwrap();
    s.clone() + &last.to_string().repeat(n - s.len())
}

#[test]
#[ignore = "deliberate: zimfile drops title words over 240 bytes; libzim aborts the whole archive (TitleIndexingError)"]
fn suggestion_handling_of_too_long_words_fails() {
    let too_long = long_word(MAX_WORD_LENGTH + 1);
    for (i, title) in [
        too_long.clone(),
        format!("Is {too_long} too long?"),
        format!(";-) {too_long}"),
        format!("too much whitespace{}", " ".repeat(MAX_WORD_LENGTH)),
        format!("too much punctuation{}", "!".repeat(MAX_WORD_LENGTH)),
    ]
    .iter()
    .enumerate()
    {
        let path = std::env::temp_dir().join(format!(
            "zimfile-libzim-xapian-too-long-{}-{i}.zim",
            std::process::id()
        ));
        let mut c = Creator::new(&path).index("en");
        let r = c
            .add_item(item("path", title, "foo"))
            .and_then(|_| c.finish());
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err(), "title: {title}");
    }
}

#[test]
fn suggestion_handling_of_too_long_words() {
    let w = long_word(MAX_WORD_LENGTH);
    let (is, twice) = (format!("Is {w} too long?"), format!("{w} {w}"));
    let z = empty_articles(
        "suggestion_handling_of_too_long_words",
        "en",
        &[("path1", &w), ("path2", &is), ("path3", &twice)],
    );
    z.suggest_all("long", &[&is]);
    z.suggest_all("awordthatis", &[&twice, &w, &is]);
}

#[test]
fn suggestion_title_edge_cases() {
    let z = empty_articles(
        "suggestion_title_edge_cases",
        "en",
        &[
            ("About", "About"),
            ("Trout", "trout"),
            ("Without", ""),
            ("winknsmilewithouttext", ";-)"),
            ("winknsmilebothways", ";-) wink'n'smile"),
            ("winknsmiletheotherwayaround", "wink'n'smile ;-)"),
            ("winknsmilewithothernonwords", "~~ ;-) ~~"),
            ("Stout", "About Rex Stout"),
            ("Hangout", "Without a trout"),
        ],
    );
    z.suggest_all("abo", &["About", "About Rex Stout"]);
    // "Without" is a path rather than a title.
    z.suggest_all("witho", &["Without", "Without a trout"]);
    z.suggest_all("tro", &["trout", "Without a trout"]);
    z.suggest_all("hang", &[]);
    // ";-)" isn't a term in the presence of anything else.
    z.suggest_all(";-", &[";-)"]);
    z.suggest_all("win", &[";-) wink'n'smile", "wink'n'smile ;-)"]);
    // "smile" in "wink'n'smile" isn't a separate term.
    z.suggest_all("smile", &[]);
}

// Suggestion.indexFullPath: skipped, zimfile has no getter for the Xapian
// documents' data (the suggestions above do resolve through it).

// ---- search.cpp ----

#[test]
fn search_index_full_path() {
    let z = build(
        "search_index_full_path",
        Some("en"),
        Some("testPath"),
        |c| {
            c.add_item(item("testPath", "Test Article", "This is a test article"))?;
            c.add_alias("anotherPath", "Another Test Article", "testPath", false)?;
            c.add_metadata("Title", b"Test zim".to_vec(), TITLE_MIME)
        },
    );
    // (getDbData's "C/" prefix: no getter in zimfile.)
    let hits = z.search_exact("test article", z.n(), &["Test Article"]);
    assert_eq!(hits[0].entry.path, "testPath");
}

// Search.fulltextSnippet: skipped, snippets.

// Search.multiSearch: multi-archive (the same archive twice). Ported for one
// archive: the counts, and that a shorter page is a prefix of a longer one.
#[test]
fn search_multi_search_one_archive() {
    let z = build(
        "search_multi_search_one_archive",
        Some("en"),
        Some("path0"),
        |c| {
            for (p, t, body) in [
                ("path0", "Test Article0", "This is a test article. temp0"),
                (
                    "path1",
                    "Test Article1",
                    "This is another test article. For article1.",
                ),
                (
                    "path2",
                    "Test Article001",
                    "This is a test article. Super. temp0",
                ),
                ("path3", "Test Article2", "This is a test article. Super."),
                ("path4", "Test Article23", "This is a test article. bis."),
            ] {
                c.add_item(item(p, t, body))?;
            }
            // Should not be fulltext indexed
            c.add_alias("path5", "Test Article5", "path0", false)
        },
    );
    let five = z.search("test article", 100);
    assert_eq!(five.len(), 5);
    let two = z.search("test article", 2);
    assert_eq!(two, five[..2]);
    assert_eq!(z.search("test article", 3).len(), 3);
    assert_eq!(z.search("super", 100).len(), 2);
    assert_eq!(z.search("temp0", 100).len(), 2);
}

#[test]
fn search_no_ft_index() {
    let z = build("search_no_ft_index", None, Some("path0"), |c| {
        c.add_item(item(
            "path0",
            "Test Article0",
            "This is a test article. temp0",
        ))
    });
    assert!(!z.a.has_fulltext_index());
    assert!(z.a.search("test article", 10).is_err());
}

#[test]
fn search_no_stemming() {
    let z = build("search_no_stemming", Some("nostem"), Some("path0"), |c| {
        c.add_item(item(
            "path0",
            "Test Article0",
            "This is a test article. temp0",
        ))?;
        c.add_item(item(
            "path1",
            "Test Article1",
            "This is another test article. For article1.",
        ))
    });
    assert_eq!(z.search("test article", 100).len(), z.n());
    assert_eq!(z.search("test article", 1)[0].entry.title, "Test Article0");
}

// Search.geoQuery: skipped, geo queries.

#[test]
fn search_cjk() {
    let z = build("search_cjk", Some("zh"), Some("path0"), |c| {
        c.add_item(item(
            "path0",
            "Test Article0",
            "This is a test article. 平方",
        ))?;
        c.add_item(item(
            "path1",
            "Test Article1",
            "This is another test article. 平方根",
        ))
    });
    // `平方` must also find `平方根`.
    assert_eq!(z.search("平方", 100).len(), 2);
    assert_eq!(z.search("平方", 1)[0].entry.title, "Test Article0");
    assert_eq!(z.search("平方根", 100).len(), 1);
    assert_eq!(z.search("平方根", 1)[0].entry.title, "Test Article1");
}

#[test]
fn search_accents() {
    let z = build("search_accents", Some("nostem"), Some("path0"), |c| {
        c.add_item(item(
            "path0",
            "Test Article0",
            "This is a tèst articlé. temp0",
        ))?;
        c.add_item(item(
            "path1",
            "Test Article1",
            "This is another test article. For article1.",
        ))
    });
    for q in ["test article", "test àrticlé"] {
        assert_eq!(z.search(q, 100).len(), z.n(), "{q}");
        assert_eq!(z.search(q, 1)[0].entry.title, "Test Article0", "{q}");
    }
}

// ---- search_iterator.cpp ----

// search_iterator.uninitialized, .end, .copy: skipped, iterator API (the
// search behind .end and .copy is the one in .iteration).

#[test]
fn search_iterator_functions() {
    let z = from_content(
        "search_iterator_functions",
        &[
            ("item a", "item item item"),
            ("Item B", "item item 2"),
            ("iTem ć", "item number 3"), // forcing an order using wdf
        ],
    );
    let hits = z.search_exact("item", z.n(), &["item a", "Item B", "iTem ć"]);
    let h = &hits[0];
    assert_eq!(h.entry.path, "dummyPathitem a");
    assert_eq!(h.percent, 100); // getScore
    assert_eq!(h.word_count, Some(3));
    // getFileIndex, getZimId: no getters in zimfile.
}

#[test]
fn search_iterator_iteration() {
    let z = from_content(
        "search_iterator_iteration",
        &[
            ("article 1", "item"),
            ("article 2", "another item in article 2"), // different wdf
        ],
    );
    z.search_exact("item", z.n(), &["article 1", "article 2"]);
}
