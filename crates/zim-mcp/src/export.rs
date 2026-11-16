//! Export articles as Markdown files with attribution front matter.
//! Layout: `<out>/<archive-name>/<shard>/<entry-path>.md`. Never writes to
//! the archive (libzim opens it read-only).

use crate::{library, markdown, zim};
use std::fs;
use std::path::Path;

#[derive(Debug, Default)]
pub struct Stats {
    pub written: u64,
    pub redirects: u64,
    pub skipped_existing: u64,
    pub skipped_non_article: u64,
}

/// What the archive's metadata says about its license. Wikimedia projects
/// don't set `License`, so their `Source` domain stands in for it.
fn license(lib: &zim::Library) -> Option<String> {
    let l = lib.metadata(0, "License");
    if !l.is_empty() {
        return Some(l);
    }
    let source = lib.metadata(0, "Source");
    if source.ends_with("wikinews.org") {
        Some("CC BY 2.5".into())
    } else if [
        "wikipedia.org",
        "wiktionary.org",
        "wikivoyage.org",
        "wikibooks.org",
        "wikiquote.org",
        "wikisource.org",
        "wikiversity.org",
    ]
    .iter()
    .any(|d| source.ends_with(d))
    {
        Some("CC BY-SA 4.0".into())
    } else {
        None
    }
}

fn yaml(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ")
    )
}

struct Ctx<'a> {
    lib: &'a zim::Library,
    name: String,
    date: String,
    source: String,
    creator: String,
    license: Option<String>,
    out: std::path::PathBuf,
}

impl Ctx<'_> {
    fn front_matter(&self, title: &str, path: &str) -> String {
        let source = if self.source.is_empty() {
            String::new()
        } else if self.lib.metadata(0, "Scraper").starts_with("mwoffliner") {
            format!("https://{}/wiki/{}", self.source, path)
        } else {
            self.source.clone()
        };
        let mut s = format!(
            "---\ntitle: {}\narchive: {}\npath: {}\ndate: {}\nsource: {}\n",
            yaml(title),
            yaml(&self.name),
            yaml(path),
            yaml(&self.date),
            yaml(&source)
        );
        if let Some(l) = &self.license {
            let who = if self.creator.is_empty() {
                String::new()
            } else {
                format!("{} contributors, ", self.creator)
            };
            s += &format!(
                "license: {}\n",
                yaml(&format!(
                    "{l}. Text by {who}{source}. Converted from a Kiwix ZIM archive."
                ))
            );
        }
        s + "---\n\n"
    }

    /// Write one entry. Returns false if it isn't an article.
    fn write(&self, e: &zim::EntryInfo, stats: &mut Stats) -> Result<(), String> {
        if !e.is_redirect && !e.mimetype.starts_with("text/html") {
            stats.skipped_non_article += 1;
            return Ok(());
        }
        let file = self.out.join(markdown::export_path(&e.path));
        if file.exists() {
            // Resume: skip what's already exported. Otherwise it's another entry whose name
            // differs only in case (macOS, Windows, exFAT): redirects give way, articles win.
            // ponytail: two articles differing only in case keep the last one written.
            let existing = fs::read_to_string(&file).unwrap_or_default();
            if existing.lines().nth(3) == Some(&format!("path: {}", yaml(&e.path))) || e.is_redirect
            {
                stats.skipped_existing += 1;
                return Ok(());
            }
        }
        let body = if e.is_redirect {
            stats.redirects += 1;
            let target = self
                .lib
                .entry(0, &e.redirect_path)
                .map(|t| t.title)
                .unwrap_or_else(|| e.redirect_path.clone());
            format!(
                "Redirect to [{}]({})\n",
                target,
                markdown::relative_md_link(&e.path, &e.redirect_path)
            )
        } else {
            stats.written += 1;
            let html = self
                .lib
                .content(0, &e.path)
                .map_err(|err| format!("{}: {err}", e.path))?;
            let from = e.path.clone();
            let link = move |href: &str| match markdown::resolve_internal(&from, href) {
                Some(target) => Some(markdown::relative_md_link(&from, &target)),
                None if href.starts_with("http://") || href.starts_with("https://") => {
                    Some(href.to_string())
                }
                None => None,
            };
            markdown::to_markdown(&String::from_utf8_lossy(&html), Box::new(link)) + "\n"
        };
        let mut text = self.front_matter(&e.title, &e.path);
        if e.is_redirect {
            text = text.replacen(
                "---\n\n",
                &format!("redirect: {}\n---\n\n", yaml(&e.redirect_path)),
                1,
            );
        }
        text += &body;
        fs::create_dir_all(file.parent().unwrap())
            .map_err(|err| format!("{}: {err}", file.display()))?;
        // Write then rename, so an interrupted export never leaves a half file that resume would skip.
        let tmp = file.with_extension("md.tmp");
        fs::write(&tmp, text)
            .and_then(|_| fs::rename(&tmp, &file))
            .map_err(|err| format!("{}: {err}", file.display()))
    }
}

/// Export `paths` (or every entry with `all`) from `archive` into `out`.
pub fn export(
    archive: &Path,
    paths: &[String],
    all: bool,
    out: &Path,
    progress: bool,
) -> Result<Stats, String> {
    let file = archive.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let (name, file_date) = library::parse_filename(file)
        .ok_or_else(|| format!("{} isn't a .zim file", archive.display()))?;
    let mut lib = zim::Library::new();
    lib.open(name.clone(), archive)
        .map_err(|e| format!("{}: {e}", archive.display()))?;
    let date = lib.metadata(0, "Date");
    let ctx = Ctx {
        date: if date.is_empty() { file_date } else { date },
        source: lib.metadata(0, "Source"),
        creator: lib.metadata(0, "Creator"),
        license: license(&lib),
        out: out.join(&name),
        name,
        lib: &lib,
    };
    let mut stats = Stats::default();
    if all {
        let n = lib.entry_count(0);
        let bar = progress.then(|| indicatif::ProgressBar::new(n as u64));
        for i in 0..n {
            let e = lib.entry_at(0, i).map_err(|e| e.to_string())?;
            if let Err(err) = ctx.write(&e, &mut stats) {
                eprintln!("skipping {}: {err}", e.path); // one bad entry shouldn't stop millions
            }
            if let Some(b) = &bar {
                b.inc(1);
            }
        }
        if let Some(b) = bar {
            b.finish();
        }
    } else {
        for p in paths {
            let e = lib
                .entry(0, p)
                .ok_or_else(|| format!("no entry {p:?} in {}", archive.display()))?;
            ctx.write(&e, &mut stats)?;
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    #[test]
    fn yaml_quotes() {
        assert_eq!(super::yaml(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }
}
