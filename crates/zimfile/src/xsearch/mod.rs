//! Search through the Xapian indexes libzim embeds in archives, the way
//! libzim (Xapian 1.4) searches them: [`Archive::search`] over
//! `X/fulltext/xapian`, [`Archive::suggest`] over `X/title/xapian`.

#[allow(dead_code)] // the writer uses the rest (W3)
pub mod html;
#[allow(dead_code)] // the writer uses the rest (W3)
pub mod index;
pub mod matcher;
pub mod query;
pub mod text;

use crate::glass::Db;
use crate::{Archive, Entry, Error, Result};
use matcher::Match;
use text::{Stemmer, parse_terms, remove_accents};

/// One full-text search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub entry: Entry,
    /// BM25 weight, as Xapian computes it.
    pub score: f64,
    /// Xapian's percentage relevance.
    pub percent: u8,
    /// The article's word count, if the index stores it.
    pub word_count: Option<u32>,
}

/// The index's stemmer: its `language` metadata, else the archive's.
fn db_stemmer(db: &Db, archive: &Archive) -> Option<Stemmer> {
    let lang = match db.metadata("language").ok().flatten() {
        Some(l) if !l.is_empty() => l,
        _ => archive.metadata("Language").unwrap_or_default(),
    };
    let lang = String::from_utf8_lossy(&lang);
    text::stemmer(lang.split(',').next().unwrap_or(""))
}

fn exists(db: &Db) -> impl Fn(&str) -> bool + '_ {
    |t| db.freqs(t.as_bytes()).is_ok_and(|f| f.0 > 0)
}

/// The entry a document's data (`C/<path>`) names.
fn entry(archive: &Archive, db: &Db, did: u32) -> Result<Option<Entry>> {
    let Some(data) = db.data(did)? else {
        return Ok(None);
    };
    Ok(archive.entry_by_path(&String::from_utf8_lossy(&data)))
}

impl Archive {
    /// Full-text search of the embedded index ([`has_fulltext_index`](Self::has_fulltext_index)):
    /// the best `limit` articles for `q`, as libzim ranks them.
    pub fn search(&self, q: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let db = self
            .xapian(true)
            .ok_or_else(|| Error::Search("the archive has no full-text index".into()))?;
        let stem = db_stemmer(db, self);
        let Some(query) = query::fulltext(&remove_accents(q), stem.as_ref(), &exists(db)) else {
            return Ok(vec![]);
        };
        let (mut ms, total) = matcher::matches(db, &query, matcher::FULLTEXT)?;
        let Some(best) = ms.iter().fold(None::<Match>, |b, m| match b {
            Some(b) if b.weight >= m.weight => Some(b),
            _ => Some(*m),
        }) else {
            return Ok(vec![]);
        };
        // By weight, then docid (the input order: the sort is stable).
        ms.sort_by(|a, b| b.weight.total_cmp(&a.weight));
        let mut hits = vec![];
        for m in ms {
            if hits.len() == limit {
                break;
            }
            let Some(e) = entry(self, db, m.did)? else {
                continue;
            };
            let word_count = db
                .value(m.did, 1)?
                .and_then(|v| String::from_utf8(v).ok()?.parse().ok());
            hits.push(SearchHit {
                entry: e,
                score: m.weight,
                percent: matcher::percent(m.weight, &best, total),
                word_count,
            });
        }
        Ok(hits)
    }

    /// Title suggestions through the embedded title index, as libzim makes
    /// them (`None` if there's no usable index).
    pub(crate) fn suggest_xapian(&self, q: &str, limit: usize) -> Option<Vec<Entry>> {
        let db = self.xapian(false)?;
        let run = || -> Result<Vec<Entry>> {
            let stem = db_stemmer(db, self);
            let Some(query) = query::suggestion(&remove_accents(q), stem.as_ref(), &exists(db))
            else {
                return Ok(vec![]);
            };
            let (mut ms, _) = matcher::matches(db, &query, matcher::SUGGEST)?;
            ms.sort_by(|a, b| b.weight.total_cmp(&a.weight));
            // Ties on weight: by title (value 0), then docid. Collapse on the
            // target path (value 1). Values are read only for groups reached.
            let mut out = vec![];
            let mut seen = std::collections::HashSet::new();
            let mut i = 0;
            while i < ms.len() && out.len() < limit {
                let j = i + ms[i..]
                    .iter()
                    .take_while(|m| m.weight == ms[i].weight)
                    .count();
                let mut group = vec![];
                for m in &ms[i..j] {
                    group.push((db.value(m.did, 0)?.unwrap_or_default(), m.did));
                }
                group.sort();
                for (_, did) in group {
                    let key = db.value(did, 1)?.unwrap_or_default();
                    if !key.is_empty() && !seen.insert(key) {
                        continue;
                    }
                    if out.len() < limit
                        && let Some(e) = entry(self, db, did)?
                    {
                        out.push(e);
                    }
                }
                i = j;
            }
            Ok(out)
        };
        run().ok()
    }
}

/// A plain-text extract of an article's HTML around the first words of `q`
/// (about 200 characters).
// ponytail: the first window holding a query word, not Xapian's snippet
// algorithm (MSet::snippet); port that if matching Kiwix's snippets matters.
pub fn snippet(html: &str, q: &str) -> String {
    const CHARS: usize = 200;
    let text = html::parse(html).dump;
    let mut words = vec![];
    parse_terms(&remove_accents(q), true, false, |t, _| {
        words.push(t.to_string());
        true
    });
    let starts: Vec<usize> = text
        .char_indices()
        .filter(|&(i, c)| {
            !c.is_whitespace() && (i == 0 || text[..i].ends_with(char::is_whitespace))
        })
        .map(|(i, _)| i)
        .collect();
    let hit = starts.iter().position(|&s| {
        let w: String = text[s..]
            .chars()
            .take_while(|c| !c.is_whitespace())
            .collect();
        let w = remove_accents(w.trim_matches(|c: char| !c.is_alphanumeric()));
        words
            .iter()
            .any(|q| q.len() >= 3 && w.starts_with(q.as_str()) || *q == w)
    });
    // A few words of context before the first hit.
    let from = hit.map_or(0, |h| starts[h.saturating_sub(4)]);
    let mut s: String = text[from..]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if s.chars().count() > CHARS {
        s = s.chars().take(CHARS).collect();
        if let Some(cut) = s.rfind(' ') {
            s.truncate(cut);
        }
        s.push('…');
    }
    s
}
