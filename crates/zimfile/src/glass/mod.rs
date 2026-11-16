//! Xapian's Glass database format, as libzim embeds it in ZIM archives
//! (`X/fulltext/xapian`, `X/title/xapian`): one compacted single-file
//! database per index. Read and write only what those indexes use: the
//! postlist, docdata and position tables.

#![allow(dead_code)] // until search and the writer use all of it

pub mod bitstream;
pub mod btree;
pub mod db;
pub mod pack;

pub use btree::Source;
pub use db::Db;

#[cfg(test)]
mod tests;
