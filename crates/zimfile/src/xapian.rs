//! The glue to `zimfile-xapian`: search the Xapian indexes libzim embeds in
//! archives, the way libzim (Xapian 1.4) searches them —
//! [`Archive::search`] over `X/fulltext/xapian`, [`Archive::suggest`] over
//! `X/title/xapian` — mapping their document paths to entries.

use crate::{Archive, Entry, Error, Result};

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

impl From<zimfile_xapian::Error> for Error {
    fn from(e: zimfile_xapian::Error) -> Error {
        match e {
            zimfile_xapian::Error::Io(e) => Error::Io(e),
            zimfile_xapian::Error::Format(m) => Error::Format(m),
            zimfile_xapian::Error::Invalid(m) => Error::Invalid(m),
        }
    }
}

impl Archive {
    /// `M/Language`, the stemmer's language for indexes without their own.
    fn language(&self) -> String {
        String::from_utf8_lossy(&self.metadata("Language").unwrap_or_default()).into_owned()
    }

    /// Full-text search of the embedded index ([`has_fulltext_index`](Self::has_fulltext_index)):
    /// the best `limit` articles for `q`, as libzim ranks them.
    pub fn search(&self, q: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let index = self
            .xapian(true)
            .ok_or_else(|| Error::Search("the archive has no full-text index".into()))?;
        Ok(index
            .search(q, limit, &self.language())?
            .into_iter()
            .filter_map(|h| {
                Some(SearchHit {
                    entry: self.entry_by_path(&h.path)?,
                    score: h.score,
                    percent: h.percent,
                    word_count: h.word_count,
                })
            })
            .collect())
    }

    /// Title suggestions through the embedded title index, as libzim makes
    /// them (`None` if there's no usable index).
    pub(crate) fn suggest_xapian(&self, q: &str, limit: usize) -> Option<Vec<Entry>> {
        let paths = self
            .xapian(false)?
            .suggest(q, limit, &self.language())
            .ok()?;
        Some(paths.iter().filter_map(|p| self.entry_by_path(p)).collect())
    }
}
