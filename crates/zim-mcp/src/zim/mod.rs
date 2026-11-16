//! libzim binding: a cxx bridge to a small C++ shim (shim.cpp).
//! A `Library` holds open archives; everything else addresses them by index.

#[cxx::bridge]
mod ffi {
    #[derive(Debug, Clone, Default)]
    struct EntryInfo {
        path: String,
        title: String,
        is_redirect: bool,
        redirect_path: String,
        mimetype: String,
    }

    #[derive(Debug, Clone)]
    struct Hit {
        archive: usize,
        path: String,
        title: String,
        snippet: String,
        score: i32,
    }

    unsafe extern "C++" {
        include!("zim-mcp/src/zim/shim.h");
        type Library;
        fn new_library() -> UniquePtr<Library>;
        fn open_archive(lib: Pin<&mut Library>, path: &str) -> Result<usize>;
        fn metadata(lib: &Library, archive: usize, name: &str) -> Result<String>;
        fn article_count(lib: &Library, archive: usize) -> Result<u32>;
        fn entry_count(lib: &Library, archive: usize) -> Result<u32>;
        fn has_fulltext(lib: &Library, archive: usize) -> Result<bool>;
        fn entry_by_path(lib: &Library, archive: usize, path: &str) -> Result<EntryInfo>;
        fn entry_at(lib: &Library, archive: usize, index: u32) -> Result<EntryInfo>;
        fn content(lib: &Library, archive: usize, path: &str) -> Result<Vec<u8>>;
        fn fulltext_search(
            lib: &Library,
            archives: &[usize],
            query: &str,
            limit: u32,
        ) -> Result<Vec<Hit>>;
        // The parity test's oracle.
        #[allow(dead_code)]
        fn title_range(
            lib: &Library,
            archive: usize,
            query: &str,
            limit: u32,
        ) -> Result<Vec<String>>;
        #[allow(dead_code)]
        fn has_title_index(lib: &Library, archive: usize) -> Result<bool>;
        #[allow(dead_code)]
        fn all_entry_count(lib: &Library, archive: usize) -> Result<u32>;
        #[allow(dead_code)]
        fn main_path(lib: &Library, archive: usize) -> Result<String>;
        #[allow(dead_code)]
        fn content_at(lib: &Library, archive: usize, index: u32) -> Result<Vec<u8>>;
        fn suggest(lib: &Library, archive: usize, query: &str, limit: u32) -> Result<Vec<Hit>>;
    }
}

pub use ffi::{EntryInfo, Hit};

#[cfg(test)]
mod parity;
use std::path::Path;

pub type Error = cxx::Exception;

/// One open archive, identified by its name (filename minus `_YYYY-MM.zim`).
#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    pub name: String,
}

/// Open ZIM archives. Index `i` in `archives` is libzim archive `i`.
pub struct Library {
    inner: cxx::UniquePtr<ffi::Library>,
    pub archives: Vec<ArchiveInfo>,
}

impl Library {
    pub fn new() -> Self {
        Library {
            inner: ffi::new_library(),
            archives: vec![],
        }
    }

    pub fn open(&mut self, name: String, file: &Path) -> Result<usize, Error> {
        let i = ffi::open_archive(self.inner.pin_mut(), &file.to_string_lossy())?;
        self.archives.push(ArchiveInfo { name });
        Ok(i)
    }

    fn lib(&self) -> &ffi::Library {
        self.inner.as_ref().expect("library")
    }

    pub fn metadata(&self, a: usize, name: &str) -> String {
        ffi::metadata(self.lib(), a, name).unwrap_or_default()
    }
    pub fn article_count(&self, a: usize) -> u32 {
        ffi::article_count(self.lib(), a).unwrap_or(0)
    }
    pub fn entry_count(&self, a: usize) -> u32 {
        ffi::entry_count(self.lib(), a).unwrap_or(0)
    }
    pub fn has_fulltext(&self, a: usize) -> bool {
        ffi::has_fulltext(self.lib(), a).unwrap_or(false)
    }
    pub fn entry(&self, a: usize, path: &str) -> Option<EntryInfo> {
        ffi::entry_by_path(self.lib(), a, path).ok()
    }
    pub fn entry_at(&self, a: usize, index: u32) -> Result<EntryInfo, Error> {
        ffi::entry_at(self.lib(), a, index)
    }
    /// Content of the entry at `path`, following redirects.
    pub fn content(&self, a: usize, path: &str) -> Result<Vec<u8>, Error> {
        ffi::content(self.lib(), a, path)
    }
    pub fn fulltext_search(
        &self,
        archives: &[usize],
        query: &str,
        limit: u32,
    ) -> Result<Vec<Hit>, Error> {
        ffi::fulltext_search(self.lib(), archives, query, limit)
    }
    pub fn suggest(&self, a: usize, query: &str, limit: u32) -> Result<Vec<Hit>, Error> {
        ffi::suggest(self.lib(), a, query, limit)
    }
}
