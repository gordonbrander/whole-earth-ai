//! Read an archive and write it back with `Creator` (items, redirects,
//! metadata, main page, uuid), then check that every `C` entry survived:
//! path, title, mimetype, redirect target and content SHA-256.
//!
//! `cargo run --release --example repack -- in.zim out.zim [--index eng]`
//! (`--index`: embed Xapian full-text and title indexes, in that language).

use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use zimfile::{Archive, Content, Creator, Item};

fn main() -> zimfile::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (src, dst, lang) = match &args[..] {
        [s, d] => (s, d, None),
        [s, d, flag, l] if flag == "--index" => (s, d, Some(l.as_str())),
        _ => {
            eprintln!("usage: repack <in.zim> <out.zim> [--index <lang>]");
            std::process::exit(2);
        }
    };
    let (src, dst) = (PathBuf::from(src), PathBuf::from(dst));
    let a = Archive::open(&src)?;
    repack(&a, &dst, lang)?;
    let (want, got) = (entries(&a)?, entries(&Archive::open(&dst)?)?);
    let same = want.iter().zip(&got).filter(|(w, g)| w == g).count();
    println!("{same} of {} C entries identical", want.len());
    if want != got {
        let diff = want.iter().zip(&got).find(|(w, g)| w != g);
        eprintln!(
            "first difference: {diff:?}; counts {} vs {}",
            want.len(),
            got.len()
        );
        std::process::exit(1);
    }
    Ok(())
}

// ponytail: new-namespace archives only (every archive since 2021); old ones
// would need their `A/`, `I/`, … paths mapped onto `C/`.
fn repack(a: &Archive, dst: &Path, index: Option<&str>) -> zimfile::Result<()> {
    if !a.header().new_namespaces() {
        return Err(zimfile::Error::Invalid(
            "old-namespace archives aren't supported".into(),
        ));
    }
    let mut c = Creator::new(dst).uuid(a.uuid());
    if let Some(lang) = index {
        c = c.index(lang);
    }
    if let Some(main) = a.main_entry() {
        c = c.main_path(&a.resolve(&main)?.path);
    }
    let front: HashSet<u32> = if a.has_front_articles_index() {
        a.front_articles()?.into_iter().collect()
    } else {
        HashSet::new()
    };
    for k in a.metadata_keys() {
        if k == "Counter" {
            continue; // the creator writes its own
        }
        if let Some(e) = a.metadata_entry(&k) {
            c.add_metadata(&k, a.content(&e)?, &e.mimetype)?;
        }
    }
    for i in a.by_path() {
        let e = a.entry_at(i)?;
        match e.redirect {
            Some(t) => {
                let t = a.entry_at(t)?;
                c.add_redirection(&e.path, &e.title, &t.path, front.contains(&i))?;
            }
            None => {
                let mut item = Item::new(
                    &e.path,
                    &e.title,
                    &e.mimetype,
                    Content::Bytes(a.content(&e)?),
                );
                item.front = Some(front.contains(&i));
                c.add_item(item)?;
            }
        }
    }
    c.finish()
}

/// (path, title, mimetype, redirect target path, content SHA-256).
type Row = (String, String, String, String, Vec<u8>);

/// One row per `C` entry, in path order.
fn entries(a: &Archive) -> zimfile::Result<Vec<Row>> {
    a.by_path()
        .map(|i| {
            let e = a.entry_at(i)?;
            let (target, sha) = match e.redirect {
                Some(t) => (a.entry_at(t)?.path, vec![]),
                None => (String::new(), Sha256::digest(a.content(&e)?).to_vec()),
            };
            Ok((e.path, e.title, e.mimetype, target, sha))
        })
        .collect()
}
