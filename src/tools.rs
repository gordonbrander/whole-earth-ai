//! The read-only, offline operations behind both front ends (MCP and CLI):
//! search, read and list. Built on `zim` + `markdown`.

use crate::{library, markdown, zim};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

const SNIPPET_CHARS: usize = 200;
const PAGE_BYTES: usize = 3000;

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub archive: String,
    pub title: String,
    pub id: String,
    pub snippet: String,
}

#[derive(Debug, Serialize)]
pub struct Article {
    pub title: String,
    pub archive: String,
    pub id: String,
    pub page: usize,
    pub pages: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Section headings; only on page 1 of a whole article.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outline: Vec<String>,
    pub markdown: String,
}

#[derive(Debug, Serialize)]
pub struct ArchiveSummary {
    pub name: String,
    pub title: String,
    pub language: String,
    pub date: String,
    pub articles: u32,
    pub has_fulltext: bool,
}

/// Open the newest version of each archive in `dir`, or only `only`.
fn open(dir: &Path, only: Option<&str>) -> Result<zim::Library, String> {
    let mut lib = zim::Library::new();
    let archives =
        library::newest(dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?;
    for a in archives
        .into_iter()
        .filter(|a| only.is_none_or(|n| n == a.name))
    {
        // A damaged file shouldn't take the other archives down with it.
        if let Err(e) = lib.open(a.name.clone(), &a.path) {
            eprintln!("zim-mcp: skipping {}: {e}", a.path.display());
        }
    }
    match only {
        Some(n) if lib.archives.is_empty() => Err(format!(
            "no archive named {n:?}; use list_archives to see the names"
        )),
        _ => Ok(lib),
    }
}

pub fn search(
    dir: &Path,
    query: &str,
    archive: Option<&str>,
    limit: u32,
) -> Result<Vec<SearchHit>, String> {
    let lib = open(dir, archive)?;
    let limit = limit.clamp(1, 50);
    // libzim searches archives together only when they share a language.
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut no_index = vec![];
    for a in 0..lib.archives.len() {
        if lib.has_fulltext(a) {
            groups
                .entry(lib.metadata(a, "Language"))
                .or_default()
                .push(a);
        } else {
            no_index.push(a);
        }
    }
    let mut hits = vec![];
    for archives in groups.values() {
        match lib.fulltext_search(archives, query, limit) {
            Ok(h) => hits.extend(h),
            Err(_) => no_index.extend(archives), // index unreadable: fall back to titles
        }
    }
    for a in no_index {
        hits.extend(lib.suggest(a, query, limit).unwrap_or_default());
    }
    hits.sort_by_key(|h| -h.score); // stable: title suggestions keep their order
    hits.truncate(limit as usize);
    Ok(hits
        .into_iter()
        .map(|h| {
            let name = &lib.archives[h.archive].name;
            let snippet =
                markdown::truncate(markdown::strip_tags(&h.snippet).trim(), SNIPPET_CHARS);
            SearchHit {
                archive: name.clone(),
                id: format!("{name}/{}", h.path),
                title: h.title,
                snippet,
            }
        })
        .collect())
}

/// Split an article ID (`<archive>/<entry-path>`).
pub fn parse_id(id: &str) -> Result<(&str, &str), String> {
    id.split_once('/')
        .filter(|(a, p)| !a.is_empty() && !p.is_empty())
        .ok_or_else(|| {
            format!("bad article id {id:?}: expected <archive>/<path>, as returned by search")
        })
}

pub fn read(dir: &Path, id: &str, page: usize, section: Option<&str>) -> Result<Article, String> {
    let (name, path) = parse_id(id)?;
    let lib = open(dir, Some(name))?;
    let mut entry = lib
        .entry(0, path)
        .ok_or_else(|| format!("no article {id:?}"))?;
    // Report the target of a redirect, so the ID the model quotes is canonical.
    for _ in 0..10 {
        if !entry.is_redirect {
            break;
        }
        entry = lib
            .entry(0, &entry.redirect_path)
            .ok_or_else(|| format!("broken redirect in {id:?}"))?;
    }
    if !entry.mimetype.starts_with("text/html") {
        return Err(format!("{id:?} is {}, not an article", entry.mimetype));
    }
    let html = lib.content(0, &entry.path).map_err(|e| e.to_string())?;
    // Links become plain text: they would double the size for a small model.
    let md = markdown::to_markdown(&String::from_utf8_lossy(&html), Box::new(|_| None));
    let outline = markdown::outline(&md);
    let body = match section {
        Some(s) => markdown::section(&md, s).ok_or_else(|| {
            format!(
                "no section {s:?} in {id:?}; sections: {}",
                outline.join(", ")
            )
        })?,
        None => md,
    };
    let pages = markdown::paginate(&body, PAGE_BYTES);
    let page = page.max(1);
    let text = pages
        .get(page - 1)
        .ok_or_else(|| format!("{id:?} has {} pages", pages.len()))?;
    Ok(Article {
        title: entry.title,
        archive: name.to_string(),
        id: format!("{name}/{}", entry.path),
        page,
        pages: pages.len(),
        outline: if page == 1 && section.is_none() {
            outline
        } else {
            vec![]
        },
        section: section.map(String::from),
        markdown: text.clone(),
    })
}

pub fn list_archives(dir: &Path) -> Result<Vec<ArchiveSummary>, String> {
    let lib = open(dir, None)?;
    let dates: BTreeMap<String, String> = library::newest(dir)
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.name, a.date))
        .collect();
    Ok((0..lib.archives.len())
        .map(|a| {
            let name = lib.archives[a].name.clone();
            let date = lib.metadata(a, "Date");
            ArchiveSummary {
                title: lib.metadata(a, "Title"),
                language: lib.metadata(a, "Language"),
                date: if date.is_empty() {
                    dates.get(&name).cloned().unwrap_or_default()
                } else {
                    date
                },
                articles: lib.article_count(a),
                has_fulltext: lib.has_fulltext(a),
                name,
            }
        })
        .collect())
}

// Markdown renderings, shared by the CLI and the MCP tools.

pub fn hits_md(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "No results.".into();
    }
    hits.iter()
        .enumerate()
        .map(|(i, h)| {
            format!(
                "{}. **{}** — id: `{}`\n   {}",
                i + 1,
                h.title,
                h.id,
                h.snippet
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n\nSnippets are partial: read the most relevant article before answering."
}

pub fn article_md(a: &Article) -> String {
    let mut s = format!(
        "# {}\n\nid: `{}` · archive: {} · page {} of {}",
        a.title, a.id, a.archive, a.page, a.pages
    );
    if let Some(sec) = &a.section {
        s += &format!(" · section: {sec}");
    }
    if !a.outline.is_empty() {
        s += &format!("\nsections: {}", a.outline.join(" | "));
    }
    s += &format!("\n\n{}", a.markdown);
    if a.page < a.pages {
        s += &format!("\n\n[continues: read id `{}` page {}]", a.id, a.page + 1);
    }
    s
}

pub fn archives_md(list: &[ArchiveSummary]) -> String {
    if list.is_empty() {
        return "No archives.".into();
    }
    list.iter()
        .map(|a| {
            format!(
                "- **{}**: {} · lang {} · {} · {} articles{}",
                a.name,
                a.title,
                a.language,
                a.date,
                a.articles,
                if a.has_fulltext {
                    ""
                } else {
                    " · titles only"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
