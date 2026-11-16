//! Search over libzim's Xapian indexes: the text pipeline, query parsing
//! and matching that libzim (Xapian 1.4) does.

#![allow(dead_code)] // until search and the writer use all of it

pub mod index;
pub mod text;
pub mod html;
