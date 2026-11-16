//! Read and write ZIM archives (the format behind Kiwix and openZIM), in pure
//! Rust. A port of libzim 9.8.2's reader and writer (everything but Xapian).
//!
//! - [`Archive`] opens a `.zim` (or a split `.zimaa`, `.zimab`, …, or byte
//!   ranges of other files) and looks entries up by path, index or title,
//!   over path, title and cluster-order ranges, with libzim's rules; reads
//!   content whole or in slices; lists illustrations; runs libzim's
//!   integrity [`Check`]s.
//! - [`Creator`] writes archives that libzim reads, streaming
//!   [`Content::File`] items from disk.
//! - [`search`] builds and queries a tantivy full-text index per archive, kept
//!   in a hidden `.zimfile/` folder beside the archives.

mod file;
pub mod format;
pub(crate) mod glass;
mod illustration;
mod reader;
pub mod search;
mod writer;
pub(crate) mod xsearch;

pub use format::Compression;
pub use illustration::Illustration;
pub use reader::{Archive, Check, Entry, validate};
pub use writer::{Content, Creator, Item};
pub use xsearch::{SearchHit, snippet};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The file isn't a valid ZIM archive.
    Format(String),
    /// The writer was given something it can't write.
    Invalid(String),
    /// From the full-text index.
    Search(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Format(m) | Error::Invalid(m) | Error::Search(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        match e.downcast::<Error>() {
            Ok(inner) => inner,
            Err(e) => Error::Io(e),
        }
    }
}
