//! The library: the archive folder *is* the state. Versions come from
//! filenames (`<name>_<YYYY-MM>.zim`); downloads in progress are
//! `<file>.part` with the `.meta4` saved beside them. No database.

pub mod catalog;
pub mod download;

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct LocalArchive {
    pub name: String,
    /// `YYYY-MM`, or empty if the filename has no date.
    pub date: String,
    pub path: PathBuf,
}

/// Split `wikipedia_en_all_maxi_2026-02.zim` into `("wikipedia_en_all_maxi", "2026-02")`.
pub fn parse_filename(file: &str) -> Option<(String, String)> {
    let stem = file.strip_suffix(".zim")?;
    if let Some((name, date)) = stem.rsplit_once('_') {
        let b = date.as_bytes();
        if b.len() == 7 && b[4] == b'-' && date.chars().filter(|c| c.is_ascii_digit()).count() == 6
        {
            return Some((name.to_string(), date.to_string()));
        }
    }
    Some((stem.to_string(), String::new()))
}

/// Every `.zim` in `dir` (never `.part` files), sorted by name then date.
pub fn scan(dir: &Path) -> std::io::Result<Vec<LocalArchive>> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir)? {
        let path = e?.path();
        let file = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
        if let Some((name, date)) = parse_filename(file) {
            out.push(LocalArchive { name, date, path });
        }
    }
    out.sort_by(|a, b| (&a.name, &a.date).cmp(&(&b.name, &b.date)));
    Ok(out)
}

/// The newest version of each archive in `dir`.
pub fn newest(dir: &Path) -> std::io::Result<Vec<LocalArchive>> {
    let mut all = scan(dir)?;
    all.reverse();
    all.dedup_by(|a, b| a.name == b.name); // keeps the first (newest) of each run
    all.reverse();
    Ok(all)
}

/// Delete an archive file and its search index (built or partial).
pub fn delete_archive(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)?;
    zimfile::search::delete(path).map_err(std::io::Error::other)
}

/// Build the archive's full-text index (in the folder's hidden `.zimfile/`)
/// unless it has one embedded or a valid one exists.
pub fn index(path: &Path, show_progress: bool) -> Result<(), String> {
    let err = |e: zimfile::Error| format!("{}: {e}", path.display());
    let archive = zimfile::Archive::open(path).map_err(err)?;
    if archive.has_fulltext_index() || zimfile::search::open(&archive, path).is_some() {
        return Ok(());
    }
    let bar = show_progress.then(|| {
        let b = indicatif::ProgressBar::new(0);
        b.set_style(
            indicatif::ProgressStyle::with_template(
                "{msg} {wide_bar} {pos}/{len} articles eta {eta}",
            )
            .unwrap(),
        );
        b.set_message(format!(
            "indexing {}",
            path.file_name().unwrap_or_default().display()
        ));
        b
    });
    zimfile::search::build(&archive, path, |done, total| {
        if let Some(b) = &bar {
            b.set_length(total as u64);
            b.set_position(done as u64);
        }
    })
    .map_err(err)?;
    if let Some(b) = bar {
        b.finish();
    }
    Ok(())
}

/// Delete every version of `name` (the `.zim` files and their indexes).
pub fn remove(dir: &Path, name: &str) -> std::io::Result<Vec<PathBuf>> {
    let files: Vec<PathBuf> = scan(dir)?
        .into_iter()
        .filter(|a| a.name == name)
        .map(|a| a.path)
        .collect();
    if files.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no archive named {name} in {}", dir.display()),
        ));
    }
    for f in &files {
        delete_archive(f)?;
    }
    Ok(files)
}

pub fn human_size(bytes: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", units[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_and_scan() {
        assert_eq!(
            parse_filename("wikipedia_en_all_maxi_2026-02.zim"),
            Some(("wikipedia_en_all_maxi".into(), "2026-02".into()))
        );
        assert_eq!(
            parse_filename("my_stuff.zim"),
            Some(("my_stuff".into(), "".into()))
        );
        assert_eq!(parse_filename("x_2026-02.zim.part"), None);

        let dir = std::env::temp_dir().join(format!("zim-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in [
            "a_2025-01.zim",
            "a_2026-02.zim",
            "b_2026-01.zim",
            "c_2026-03.zim.part",
            "c_2026-03.zim.meta4",
        ] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let n: Vec<_> = newest(&dir)
            .unwrap()
            .into_iter()
            .map(|a| (a.name, a.date))
            .collect();
        assert_eq!(
            n,
            vec![
                ("a".into(), "2026-02".into()),
                ("b".into(), "2026-01".into())
            ]
        );
        let (new, old) = (dir.join("a_2026-02.zim"), dir.join("a_2025-01.zim"));
        std::fs::create_dir_all(zimfile::search::index_dir(&new)).unwrap();
        std::fs::create_dir_all(zimfile::search::partial_dir(&old)).unwrap();
        assert_eq!(remove(&dir, "a").unwrap().len(), 2);
        assert!(!zimfile::search::index_dir(&new).exists());
        assert!(!zimfile::search::partial_dir(&old).exists());
        assert!(remove(&dir, "a").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(human_size(119 * 1024 * 1024 * 1024), "119.0 GiB");
    }
}
