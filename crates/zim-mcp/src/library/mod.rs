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

/// The newest version of each archive across `dirs`.
pub fn newest(dirs: &[PathBuf]) -> Result<Vec<LocalArchive>, String> {
    let mut all = vec![];
    for dir in dirs {
        all.extend(scan(dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?);
    }
    all.sort_by(|a, b| (&a.name, &a.date).cmp(&(&b.name, &b.date)));
    all.reverse();
    all.dedup_by(|a, b| a.name == b.name); // keeps the first (newest) of each run
    all.reverse();
    Ok(all)
}

/// Delete an archive file.
pub fn delete_archive(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
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
        let names = |dirs: &[PathBuf]| -> Vec<(String, String)> {
            newest(dirs)
                .unwrap()
                .into_iter()
                .map(|a| (a.name, a.date))
                .collect()
        };
        assert_eq!(
            names(std::slice::from_ref(&dir)),
            vec![
                ("a".into(), "2026-02".into()),
                ("b".into(), "2026-01".into())
            ]
        );
        // A second dir: its newer `b` wins, its `d` joins.
        let dir2 = dir.join("more");
        std::fs::create_dir_all(&dir2).unwrap();
        for f in ["b_2026-05.zim", "d_2026-01.zim"] {
            std::fs::write(dir2.join(f), "").unwrap();
        }
        assert_eq!(
            names(&[dir.clone(), dir2.clone()]),
            vec![
                ("a".into(), "2026-02".into()),
                ("b".into(), "2026-05".into()),
                ("d".into(), "2026-01".into())
            ]
        );
        assert!(newest(&[dir.clone(), dir.join("missing")]).is_err());
        assert_eq!(remove(&dir, "a").unwrap().len(), 2);
        assert!(remove(&dir, "a").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(human_size(119 * 1024 * 1024 * 1024), "119.0 GiB");
    }
}
