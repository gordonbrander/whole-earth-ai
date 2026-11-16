//! The documents libzim puts in its two indexes (`xapianIndexer.cpp`,
//! `xapianWorker.cpp`): terms, wdfs and positions as Xapian 1.4's term
//! generator makes them.

use super::text::{Stemmer, parse_terms, remove_accents, should_stem};
use crate::glass::write::Terms;

/// libzim's `ANCHOR_TERM`: every title starts with it, so a phrase can be
/// anchored to the start.
pub const ANCHOR: &str = "0posanchor ";
/// libzim's `MAX_INDEXABLE_TITLE_WORD_SIZE`.
pub const MAX_TITLE_WORD: usize = 240;
/// Xapian's default maximum word length.
pub const MAX_WORD: usize = 64;

fn add(terms: &mut Terms, term: &str, wdf: u32, pos: Option<u32>) {
    let e = terms.entry(term.as_bytes().to_vec()).or_default();
    e.0 += wdf;
    if let Some(p) = pos {
        e.1.push(p);
    }
}

/// A title document's terms: `"0posanchor " + removeAccents(title)` with
/// positions, STEM_SOME (the word at its position, `Z` + stem without),
/// CJK n-grams. A title of no words is indexed as one whole term instead.
pub fn title_terms(title: &str, stem: Option<&Stemmer>) -> Terms {
    let mut terms = Terms::new();
    let unaccented = remove_accents(title);
    if unaccented.is_empty() {
        return terms;
    }
    let mut pos = 0;
    parse_terms(
        &format!("{ANCHOR}{unaccented}"),
        true,
        true,
        |t, positional| {
            if t.len() > MAX_TITLE_WORD {
                return true;
            }
            if positional {
                pos += 1;
                add(&mut terms, t, 1, Some(pos));
            } else {
                add(&mut terms, t, 1, None);
            }
            if let Some(s) = stem
                && should_stem(t)
            {
                let st = s.stem(t);
                if !st.is_empty() {
                    add(&mut terms, &format!("Z{st}"), 1, None);
                }
            }
            true
        },
    );
    if terms.len() == 1 {
        terms.clear();
        if unaccented.len() <= MAX_TITLE_WORD {
            add(&mut terms, &unaccented, 1, None);
        }
    }
    terms
}

/// Index `text` without positions, STEM_ALL (stems only) with a stemmer,
/// else the words themselves; each occurrence adds `wdf`.
fn index_text(terms: &mut Terms, text: &str, wdf: u32, stem: Option<&Stemmer>) {
    parse_terms(text, true, false, |t, _| {
        if t.len() > MAX_WORD {
            return true;
        }
        match stem {
            Some(s) => {
                let st = s.stem(t);
                if !st.is_empty() {
                    add(terms, &st, wdf, None);
                }
            }
            None => add(terms, t, wdf, None),
        }
        true
    });
}

/// A full-text document's terms: the unaccented content (wdf 1 each), title
/// (`content bytes / 500 + 1` each) and keywords (3 each).
pub fn fulltext_terms(content: &str, title: &str, keywords: &str, stem: Option<&Stemmer>) -> Terms {
    let mut terms = Terms::new();
    index_text(&mut terms, content, 1, stem);
    index_text(&mut terms, title, content.len() as u32 / 500 + 1, stem);
    index_text(&mut terms, keywords, 3, stem);
    terms
}

/// libzim's `countWords`: runs of non-whitespace bytes (C `isspace`).
pub fn count_words(text: &str) -> u32 {
    text.split([' ', '\t', '\n', '\x0b', '\x0c', '\r'])
        .filter(|w| !w.is_empty())
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Archive;
    use crate::xsearch::text::stemmer;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// The climate archive's title index, inverted: (path, terms) by docid − 1.
    fn real_title_docs(a: &Archive) -> Vec<(String, Terms)> {
        let db = a.xapian(false).unwrap();
        let mut docs: BTreeMap<u32, Terms> = BTreeMap::new();
        for (t, _) in db.terms(b"").unwrap() {
            let mut pl = db.postlist(&t).unwrap().unwrap();
            while pl.next().unwrap() {
                let pos = db.positions(pl.did(), &t).unwrap();
                docs.entry(pl.did())
                    .or_default()
                    .insert(t.clone(), (pl.wdf(), pos));
            }
        }
        (1..=db.stats.doccount)
            .map(|d| {
                let path = String::from_utf8(db.data(d).unwrap().unwrap()).unwrap();
                (path, docs.remove(&d).unwrap_or_default())
            })
            .collect()
    }

    /// The archives with libzim-built indexes: climate (`just testdata`) and
    /// zim-mcp's nollywood one.
    pub fn archives() -> Vec<Archive> {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        [
            d.join("tests/data/nons/wikipedia_en_climate_change_mini_2024-06.zim"),
            d.join("../zim-mcp/tests/data/wikipedia_en_nollywood_maxi_2026-10.zim"),
        ]
        .iter()
        .map(|p| Archive::open(p).expect("test data missing: run `just testdata`"))
        .collect()
    }

    #[test]
    fn titles_match_libzim() {
        for a in archives() {
            titles_match(&a);
        }
    }

    fn titles_match(a: &Archive) {
        let st = stemmer("eng");
        let db = a.xapian(false).unwrap();
        let real = real_title_docs(a);
        let mut bad = vec![];
        for (did, (path, want)) in real.iter().enumerate() {
            let title = db.value(did as u32 + 1, 0).unwrap().unwrap();
            let got = title_terms(std::str::from_utf8(&title).unwrap(), st.as_ref());
            if &got != want {
                if bad.is_empty() {
                    let show = |t: &Terms| {
                        t.iter()
                            .map(|(k, v)| format!("{}:{:?}", String::from_utf8_lossy(k), v))
                            .collect::<Vec<_>>()
                    };
                    eprintln!("{path}\n got {:?}\nwant {:?}", show(&got), show(want));
                }
                bad.push(path.clone());
            }
        }
        assert!(
            bad.is_empty(),
            "{} of {} differ: {:?}",
            bad.len(),
            real.len(),
            &bad[..bad.len().min(10)]
        );
    }

    #[test]
    fn words() {
        assert_eq!(count_words("  a b\tc\n\n d "), 4);
        assert_eq!(count_words(""), 0);
    }
}
