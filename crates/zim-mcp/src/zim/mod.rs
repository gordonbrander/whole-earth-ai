//! Open archives through zimfile. A `Library` holds open archives; everything
//! else addresses them by index.

use std::path::Path;
use zimfile::{Archive, search};

pub type Error = zimfile::Error;

#[derive(Debug, Clone, Default)]
pub struct EntryInfo {
    pub path: String,
    pub title: String,
    pub is_redirect: bool,
    pub redirect_path: String,
    pub mimetype: String,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub archive: usize,
    pub path: String,
    pub title: String,
    pub snippet: String,
    /// BM25 for full-text hits, 0 for title suggestions.
    pub score: f32,
}

/// One open archive, identified by its name (filename minus `_YYYY-MM.zim`).
pub struct ArchiveInfo {
    pub name: String,
    archive: Archive,
    /// Its full-text index, if built.
    index: Option<search::Index>,
}

/// Open ZIM archives.
pub struct Library {
    pub archives: Vec<ArchiveInfo>,
}

impl Library {
    pub fn new() -> Self {
        Library { archives: vec![] }
    }

    pub fn open(&mut self, name: String, file: &Path) -> Result<usize, Error> {
        let archive = Archive::open(file)?;
        let index = search::open(&archive, file);
        self.archives.push(ArchiveInfo {
            name,
            archive,
            index,
        });
        Ok(self.archives.len() - 1)
    }

    fn a(&self, a: usize) -> &Archive {
        &self.archives[a].archive
    }

    /// `""` when missing.
    pub fn metadata(&self, a: usize, name: &str) -> String {
        let v = self.a(a).metadata(name).unwrap_or_default();
        String::from_utf8_lossy(&v).into_owned()
    }
    pub fn article_count(&self, a: usize) -> u32 {
        self.a(a).article_count()
    }
    /// User entries (absolute indexes `0..entry_count` cover them).
    pub fn entry_count(&self, a: usize) -> u32 {
        self.a(a).entry_count()
    }
    /// Whether the archive has a (valid) full-text index.
    pub fn has_fulltext(&self, a: usize) -> bool {
        self.archives[a].index.is_some()
    }

    fn info(&self, a: usize, e: zimfile::Entry) -> Result<EntryInfo, Error> {
        let redirect_path = match e.redirect {
            Some(t) => self.a(a).entry_at(t)?.path,
            None => String::new(),
        };
        Ok(EntryInfo {
            is_redirect: e.is_redirect(),
            path: e.path,
            title: e.title,
            redirect_path,
            mimetype: e.mimetype,
        })
    }

    pub fn entry(&self, a: usize, path: &str) -> Option<EntryInfo> {
        let e = self.a(a).entry_by_path(path)?;
        self.info(a, e).ok()
    }
    /// Entry at absolute index `index`.
    pub fn entry_at(&self, a: usize, index: u32) -> Result<EntryInfo, Error> {
        let e = self.a(a).entry_at(index)?;
        self.info(a, e)
    }
    /// Content of the entry at `path`, following redirects.
    pub fn content(&self, a: usize, path: &str) -> Result<Vec<u8>, Error> {
        let archive = self.a(a);
        let e = archive
            .entry_by_path(path)
            .ok_or_else(|| Error::Format(format!("no entry {path:?}")))?;
        archive.content(&e)
    }

    /// Full-text search of one archive's index (none: no hits).
    pub fn fulltext_search(&self, a: usize, query: &str, limit: u32) -> Result<Vec<Hit>, Error> {
        let info = &self.archives[a];
        let Some(index) = &info.index else {
            return Ok(vec![]);
        };
        Ok(index
            .search(&info.archive, query, limit as usize)?
            .into_iter()
            .map(|h| Hit {
                archive: a,
                path: h.path,
                title: h.title,
                snippet: h.snippet,
                score: h.score,
            })
            .collect())
    }

    /// Entries whose title starts with `query` (case-sensitive, as libzim without Xapian).
    pub fn suggest(&self, a: usize, query: &str, limit: u32) -> Vec<Hit> {
        self.a(a)
            .suggest(query, limit as usize)
            .into_iter()
            .map(|e| Hit {
                archive: a,
                path: e.path,
                title: e.title,
                snippet: String::new(),
                score: 0.0,
            })
            .collect()
    }
}
