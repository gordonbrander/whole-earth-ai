//! Read and write ZIM archives (the format behind Kiwix and openZIM), in pure
//! Rust. A port of libzim 9.8.2's reader and writer.
//!
//! - [`Archive`] opens a `.zim` (or a split `.zimaa`, `.zimab`, …) and looks
//!   entries up by path, index or title prefix, with libzim's rules.

mod file;
pub mod format;
mod reader;

pub use reader::{Archive, Entry};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The file isn't a valid ZIM archive.
    Format(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Format(m) => f.write_str(m),
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
