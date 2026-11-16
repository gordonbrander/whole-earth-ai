//! Full-text search through a tantivy index built from an archive's articles.
//! For archives without an embedded Xapian index: [`Archive::search`] and
//! [`Archive::suggest`] use that one when it's there.
//!
//! Indexes live in a hidden folder beside the archives, one per archive:
//!
//! ```text
//! zim/
//!   wikipedia_en_all_nopic_2026-02.zim
//!   .zimfile/index/wikipedia_en_all_nopic_2026-02.zim.idx/        built
//!   .zimfile/index/wikipedia_en_all_nopic_2026-02.zim.idx.part/   building
//! ```
//!
//! A build writes `….idx.part/` and renames it when complete, so an interrupted
//! build is never mistaken for an index. `zimfile.json` inside records the
//! archive's uuid: an index for another archive is ignored. Everything under
//! `.zimfile/` can be deleted and rebuilt.

use crate::{Archive, Error, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::directory::error::{DeleteError, OpenReadError, OpenWriteError};
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::{
    AsciiFoldingFilter, Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer,
    TextAnalyzer,
};
use tantivy::{IndexReader, TantivyDocument, doc};

const VERSION: u32 = 1;
const TOKENIZER: &str = "zim";
const SNIPPET_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub score: f32,
}

pub struct Index {
    index: tantivy::Index,
    reader: IndexReader,
    title: Field,
    body: Field,
    redirects: Field,
    path: Field,
}

fn err(e: impl std::fmt::Display) -> Error {
    Error::Search(e.to_string())
}

/// The folder of indexes for the archives in `dir`: `<dir>/.zimfile/index`.
fn index_root(dir: &Path) -> PathBuf {
    dir.join(".zimfile").join("index")
}

/// `<folder>/.zimfile/index/<file>.idx`: the index of the archive `zim`.
pub fn index_dir(zim: &Path) -> PathBuf {
    let mut name = zim.file_name().unwrap_or_default().to_owned();
    name.push(".idx");
    index_root(zim.parent().unwrap_or(Path::new(""))).join(name)
}

/// [`index_dir`] + `.part`: a build in progress (or interrupted).
pub fn partial_dir(zim: &Path) -> PathBuf {
    let mut s = index_dir(zim).into_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// Delete the index of `zim`, built or partial.
pub fn delete(zim: &Path) -> Result<()> {
    for d in [index_dir(zim), partial_dir(zim)] {
        if d.exists() {
            std::fs::remove_dir_all(d)?;
        }
    }
    Ok(())
}

/// Delete the indexes in `dir` whose archive is gone. Returns what was removed.
pub fn prune(dir: &Path) -> Result<Vec<PathBuf>> {
    let root = index_root(dir);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(vec![]); // no indexes
    };
    let mut removed = vec![];
    for e in entries {
        let path = e?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let zim = name.strip_suffix(".part").unwrap_or(&name);
        let Some(zim) = zim.strip_suffix(".idx") else {
            continue;
        };
        if !dir.join(zim).exists() {
            std::fs::remove_dir_all(&path)?;
            removed.push(path);
        }
    }
    removed.sort();
    Ok(removed)
}

/// Bytes on disk of the built index of `zim`, if there is one.
pub fn size(zim: &Path) -> Option<u64> {
    fn walk(p: &Path) -> u64 {
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| match e.metadata() {
                Ok(m) if m.is_dir() => walk(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            })
            .sum()
    }
    let d = index_dir(zim);
    d.is_dir().then(|| walk(&d))
}

fn stamp(archive: &Archive) -> String {
    let uuid: String = archive.uuid().iter().map(|b| format!("{b:02x}")).collect();
    format!("{{\"zim_uuid\": \"{uuid}\", \"version\": {VERSION}}}\n")
}

fn schema() -> (Schema, [Field; 4]) {
    let text = |stored: bool| {
        let o = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        if stored { o.set_stored() } else { o }
    };
    let mut s = Schema::builder();
    let path = s.add_text_field("path", STRING | STORED);
    let title = s.add_text_field("title", text(true));
    let body = s.add_text_field("body", text(false));
    // Titles of the redirects to this article, one per line.
    let redirects = s.add_text_field("redirects", text(false));
    (s.build(), [path, title, body, redirects])
}

/// The stemmer for the archive's `Language` (ISO 639-3, first of a list).
// ponytail: CJK isn't segmented (a run of ideographs is one token); add an
// n-gram tokenizer for zho/jpn/kor archives if they matter.
fn tokenizer(archive: &Archive) -> TextAnalyzer {
    let lang = archive.metadata("Language").unwrap_or_default();
    let lang = String::from_utf8_lossy(&lang);
    let stem = match lang.split(',').next().unwrap_or("").trim() {
        "eng" => Some(Language::English),
        "fra" => Some(Language::French),
        "deu" => Some(Language::German),
        "spa" => Some(Language::Spanish),
        "ita" => Some(Language::Italian),
        "por" => Some(Language::Portuguese),
        "rus" => Some(Language::Russian),
        "nld" => Some(Language::Dutch),
        "swe" => Some(Language::Swedish),
        "nor" | "nob" | "nno" => Some(Language::Norwegian),
        "dan" => Some(Language::Danish),
        "fin" => Some(Language::Finnish),
        "hun" => Some(Language::Hungarian),
        "ron" => Some(Language::Romanian),
        "tur" => Some(Language::Turkish),
        "ara" => Some(Language::Arabic),
        "ell" => Some(Language::Greek),
        "tam" => Some(Language::Tamil),
        _ => None,
    };
    let b = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(40))
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter);
    match stem {
        Some(l) => b.filter(Stemmer::new(l)).build(),
        None => b.build(),
    }
}

/// The articles to index — front articles (v1 title listing), or else every
/// `text/html` entry among the user entries (`A/` in old archives) — and the
/// titles of the redirects to each, by target index.
fn articles(archive: &Archive) -> Result<(Vec<crate::Entry>, HashMap<u32, String>)> {
    let front: Option<HashSet<u32>> = if archive.has_front_articles_index() {
        Some(archive.front_articles()?.into_iter().collect())
    } else {
        None
    };
    let old = !archive.header().new_namespaces();
    let mut docs = vec![];
    let mut redirects: HashMap<u32, String> = HashMap::new();
    for i in 0..archive.entry_count() {
        let Ok(e) = archive.entry_at(i) else { continue };
        if e.is_redirect() {
            if let Ok(t) = archive.resolve(&e) {
                let r = redirects.entry(t.index).or_default();
                r.push_str(&e.title);
                r.push('\n');
            }
        } else if e.mimetype.starts_with("text/html")
            && front.as_ref().is_none_or(|f| f.contains(&i))
            && (!old || e.path.starts_with("A/"))
        {
            docs.push(e);
        }
    }
    // Cluster order: each cluster is decompressed once.
    docs.sort_by_key(|e| (e.cluster, e.blob));
    Ok((docs, redirects))
}

/// Build (or rebuild) the index of `zim` ([`index_dir`]). `progress(done, total)`
/// is called as articles are indexed.
pub fn build(archive: &Archive, zim: &Path, mut progress: impl FnMut(u32, u32)) -> Result<()> {
    let part = partial_dir(zim);
    if part.exists() {
        std::fs::remove_dir_all(&part)?;
    }
    std::fs::create_dir_all(&part)?;
    let (schema, [path, title, body, redirects]) = schema();
    let index = tantivy::Index::create_in_dir(&part, schema).map_err(err)?;
    index.tokenizers().register(TOKENIZER, tokenizer(archive));
    let mut writer: tantivy::IndexWriter = index.writer(256 << 20).map_err(err)?;
    let (list, mut alt) = articles(archive)?;
    let total = list.len() as u32;
    // ponytail: HTML → text runs on this thread while tantivy indexes on its
    // own; move extraction onto scoped workers if builds turn out CPU-bound here.
    for (n, e) in list.iter().enumerate() {
        let Ok(html) = archive.content(e) else {
            continue; // one bad entry shouldn't stop the build
        };
        writer
            .add_document(doc!(
                path => e.path.as_str(),
                title => e.title.as_str(),
                body => html_text(&String::from_utf8_lossy(&html)),
                redirects => alt.remove(&e.index).unwrap_or_default(),
            ))
            .map_err(err)?;
        if n % 1000 == 0 {
            progress(n as u32, total);
        }
    }
    writer.commit().map_err(err)?;
    writer.wait_merging_threads().map_err(err)?;
    progress(total, total);
    std::fs::write(part.join("zimfile.json"), stamp(archive))?;
    let dest = index_dir(zim);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::rename(&part, &dest)?;
    Ok(())
}

/// The index of `zim`, if it exists and was built from this archive.
pub fn open(archive: &Archive, zim: &Path) -> Option<Index> {
    let dir = index_dir(zim);
    if std::fs::read_to_string(dir.join("zimfile.json")).ok()? != stamp(archive) {
        return None;
    }
    let dir = ReadOnly(tantivy::directory::MmapDirectory::open(&dir).ok()?);
    let index = tantivy::Index::open(dir).ok()?;
    index.tokenizers().register(TOKENIZER, tokenizer(archive));
    let s = index.schema();
    Some(Index {
        // Manual reload: a built index never changes.
        reader: index
            .reader_builder()
            .reload_policy(tantivy::ReloadPolicy::Manual)
            .try_into()
            .ok()?,
        title: s.get_field("title").ok()?,
        body: s.get_field("body").ok()?,
        redirects: s.get_field("redirects").ok()?,
        path: s.get_field("path").ok()?,
        index,
    })
}

/// A built index's directory, opened without lock files: an index never
/// changes once renamed into place, and it may sit on a read-only drive.
#[derive(Clone, Debug)]
struct ReadOnly(tantivy::directory::MmapDirectory);

impl tantivy::Directory for ReadOnly {
    fn get_file_handle(
        &self,
        path: &Path,
    ) -> std::result::Result<std::sync::Arc<dyn tantivy::directory::FileHandle>, OpenReadError>
    {
        self.0.get_file_handle(path)
    }
    fn delete(&self, path: &Path) -> std::result::Result<(), DeleteError> {
        self.0.delete(path)
    }
    fn exists(&self, path: &Path) -> std::result::Result<bool, OpenReadError> {
        self.0.exists(path)
    }
    fn open_write(
        &self,
        path: &Path,
    ) -> std::result::Result<tantivy::directory::WritePtr, OpenWriteError> {
        self.0.open_write(path)
    }
    fn atomic_read(&self, path: &Path) -> std::result::Result<Vec<u8>, OpenReadError> {
        self.0.atomic_read(path)
    }
    fn atomic_write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
        self.0.atomic_write(path, data)
    }
    fn sync_directory(&self) -> std::io::Result<()> {
        self.0.sync_directory()
    }
    fn watch(
        &self,
        cb: tantivy::directory::WatchCallback,
    ) -> tantivy::Result<tantivy::directory::WatchHandle> {
        self.0.watch(cb)
    }
    fn acquire_lock(
        &self,
        _lock: &tantivy::directory::Lock,
    ) -> std::result::Result<tantivy::directory::DirectoryLock, tantivy::directory::error::LockError>
    {
        Ok(tantivy::directory::DirectoryLock::from(Box::new(())))
    }
}

impl Index {
    /// Articles matching every word of `q` (title words count double), best
    /// first, with a snippet of the article text around the matches.
    pub fn search(&self, archive: &Archive, q: &str, limit: usize) -> Result<Vec<Hit>> {
        let fields = vec![self.title, self.body, self.redirects];
        let mut parser = QueryParser::for_index(&self.index, fields);
        parser.set_conjunction_by_default();
        parser.set_field_boost(self.title, 2.0);
        parser.set_field_boost(self.redirects, 2.0);
        let (query, _) = parser.parse_query_lenient(q);
        let searcher = self.reader.searcher();
        let top = searcher
            .search(&query, &TopDocs::with_limit(limit.max(1)).order_by_score())
            .map_err(err)?;
        let mut snippets = SnippetGenerator::create(&searcher, &*query, self.body).map_err(err)?;
        snippets.set_max_num_chars(SNIPPET_CHARS);
        let mut hits = vec![];
        for (score, addr) in top {
            let d: TantivyDocument = searcher.doc(addr).map_err(err)?;
            let get = |f| {
                d.get_first(f)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let path = get(self.path);
            let text = archive
                .entry_by_path(&path)
                .and_then(|e| archive.content(&e).ok())
                .map(|h| html_text(&String::from_utf8_lossy(&h)))
                .unwrap_or_default();
            let s = snippets.snippet(&text);
            let snippet = if s.fragment().is_empty() {
                text.chars().take(SNIPPET_CHARS).collect()
            } else {
                s.fragment().to_string()
            };
            hits.push(Hit {
                title: get(self.title),
                path,
                snippet,
                score,
            });
        }
        Ok(hits)
    }
}

/// Visible article text of an HTML page: tags dropped; `<script>`, `<style>`,
/// `<head>`, `<nav>` and MediaWiki navboxes and category links dropped with
/// their contents; entities decoded; whitespace collapsed.
// ponytail: a naive tag stripper; switch to an HTML tokenizer if snippet quality suffers.
pub fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        decode_into(&mut out, &rest[..i]);
        out.push(' ');
        rest = &rest[i..];
        let n = if rest.starts_with("<!--") {
            rest.find("-->").map(|j| j + 3)
        } else {
            skip_element(rest)
        };
        let n = n
            .or_else(|| rest.find('>').map(|j| j + 1))
            .unwrap_or(rest.len());
        rest = &rest[n..];
    }
    decode_into(&mut out, rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// If `s` opens an element that isn't article text, the length of the whole
/// element (to its matching close tag, counting nested ones of the same name).
fn skip_element(s: &str) -> Option<usize> {
    let end = s.find('>')? + 1;
    let tag = &s[1..end - 1];
    let name_len = tag
        .find(|c: char| c.is_ascii_whitespace() || c == '/')
        .unwrap_or(tag.len());
    let name = &tag.as_bytes()[..name_len];
    let boilerplate = ["script", "style", "head", "nav"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(str::as_bytes(n)))
        || tag.contains("navbox")
        || tag.contains("catlinks")
        || tag.contains("role=\"navigation\"");
    if !boilerplate || name.is_empty() || tag.ends_with('/') {
        return None;
    }
    let b = s.as_bytes();
    let mut depth = 1;
    let mut p = end;
    while let Some(j) = s[p..].find('<') {
        let closing = b.get(p + j + 1) == Some(&b'/');
        let q = p + j + 1 + usize::from(closing);
        let named = b.len() >= q + name.len()
            && b[q..q + name.len()].eq_ignore_ascii_case(name)
            && b.get(q + name.len())
                .is_none_or(|&c| c == b'>' || c == b'/' || c.is_ascii_whitespace());
        if named && closing {
            depth -= 1;
            if depth == 0 {
                return Some(q + s[q..].find('>')? + 1);
            }
        } else if named {
            depth += 1;
        }
        p = q;
    }
    None
}

fn decode_into(out: &mut String, mut s: &str) {
    while let Some(i) = s.find('&') {
        out.push_str(&s[..i]);
        s = &s[i..];
        let end = s[1..].find(';').map(|j| j + 1).filter(|&j| j <= 10);
        let decoded = end.and_then(|j| {
            let name = &s[1..j];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                _ => {
                    let n = name.strip_prefix('#')?;
                    let v = match n.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok()?,
                        None => n.parse().ok()?,
                    };
                    char::from_u32(v)?
                }
            };
            Some((c, j + 1))
        });
        match decoded {
            Some((c, n)) => {
                out.push(c);
                s = &s[n..];
            }
            None => {
                out.push('&');
                s = &s[1..];
            }
        }
    }
    out.push_str(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_from_html() {
        let h = "<html><head><title>T</title><style>p{}</style></head><body>\
                 <p>Fish&amp;chips &lt;3 &#233;t&#xE9; &bogus; a&b</p>\
                 <SCRIPT type=x>var x = '<p>';</SCRIPT><!-- c --><b>bold</b>text</body></html>";
        assert_eq!(html_text(h), "Fish&chips <3 été &bogus; a&b bold text");
        assert_eq!(html_text("<header>keep</header>"), "keep");
        assert_eq!(html_text("unclosed <b"), "unclosed");
        let nav = r#"a<div role="navigation" class="navbox"><div>x</div><div>y</div></div>b
                     <nav>n</nav><div id="catlinks" class="catlinks">Categories</div>c<div/>d"#;
        assert_eq!(html_text(nav), "a b c d");
    }
}
