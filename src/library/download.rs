//! Adding and updating archives. A download goes to `<file>.part` (with the
//! `.meta4` saved beside it, so `status` can read progress from the files),
//! resumes with HTTP Range, hashes while streaming, and is renamed into place
//! only once its sha256 matches. Old versions are removed only after that.

use super::catalog::{self, Meta4};
use super::{LocalArchive, human_size, newest, parse_filename};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

/// What an add would do: shown to the user for approval before it happens.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub name: String,
    pub file: String,
    pub size: u64,
    pub meta4_url: String,
    /// Older versions that are deleted once the new one checks out.
    pub replaces: Vec<String>,
    pub already_present: bool,
}

impl Plan {
    pub fn describe(&self, dir: &Path) -> String {
        if self.already_present {
            return format!("{} is already in {}.", self.file, dir.display());
        }
        let mut s = format!(
            "Download {} ({}) to {}?",
            self.file,
            human_size(self.size),
            dir.display()
        );
        if !self.replaces.is_empty() {
            s += &format!(" This replaces {}.", self.replaces.join(", "));
        }
        s
    }
}

/// `spec` is a catalog name (`wikipedia_en_all_maxi`: newest version) or a
/// URL of a `.zim` or `.zim.meta4` on a Kiwix mirror.
pub fn meta4_url(spec: &str) -> Result<String, String> {
    if spec.starts_with("http://") || spec.starts_with("https://") {
        return Ok(if spec.ends_with(".meta4") {
            spec.to_string()
        } else {
            format!("{spec}.meta4")
        });
    }
    match catalog::find(spec)? {
        Some(e) => Ok(e.meta4_url),
        None => Err(format!(
            "{spec:?} isn't in the Kiwix catalog; try `zim-mcp library search`"
        )),
    }
}

fn fetch_meta4(url: &str) -> Result<(Meta4, String), String> {
    let xml = catalog::get(url)
        .map_err(|e| format!("can't get checksum file (Kiwix publishes one per archive): {e}"))?;
    let m = catalog::parse_meta4(&xml)?;
    // The filename comes from the network: it must be a plain archive name.
    let ok = !m.file.contains(['/', '\\'])
        && !m.file.starts_with('.')
        && parse_filename(&m.file).is_some();
    if !ok {
        return Err(format!("refusing unsafe file name {:?} from {url}", m.file));
    }
    Ok((m, xml))
}

pub fn plan(dir: &Path, spec: &str) -> Result<Plan, String> {
    let url = meta4_url(spec)?;
    let (m, _) = fetch_meta4(&url)?;
    let (name, _) = parse_filename(&m.file).unwrap();
    let replaces = super::scan(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.name == name && a.path.file_name().is_some_and(|f| f != m.file.as_str()))
        .map(|a| a.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    Ok(Plan {
        already_present: dir.join(&m.file).exists(),
        name,
        file: m.file,
        size: m.size,
        meta4_url: url,
        replaces,
    })
}

/// Archives in `dir` with a newer catalog version (all of them, or only `names`).
pub fn plan_update(dir: &Path, names: &[String]) -> Result<Vec<Plan>, String> {
    let local: Vec<LocalArchive> = newest(dir).map_err(|e| e.to_string())?;
    let mut plans = vec![];
    for a in local
        .iter()
        .filter(|a| names.is_empty() || names.contains(&a.name))
    {
        if let Some(e) = catalog::find(&a.name)?
            && e.date > a.date
        {
            plans.push(plan(dir, &e.meta4_url)?);
        }
    }
    Ok(plans)
}

/// Download, verify and install an archive. With `replace`, delete the
/// archive's other versions afterwards. Returns the installed path.
pub fn add(dir: &Path, spec: &str, replace: bool, show_progress: bool) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (m, xml) = fetch_meta4(&meta4_url(spec)?)?;
    let dest = dir.join(&m.file);
    if !dest.exists() {
        download(dir, &m, &xml, show_progress)?;
    } else if show_progress {
        eprintln!("{} is already present", m.file);
    }
    if replace {
        let (name, _) = parse_filename(&m.file).unwrap();
        for old in super::scan(dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.name == name && a.path != dest)
        {
            fs::remove_file(&old.path).map_err(|e| format!("{}: {e}", old.path.display()))?;
            if show_progress {
                eprintln!("removed {}", old.path.display());
            }
        }
    }
    Ok(dest)
}

fn download(dir: &Path, m: &Meta4, meta4_xml: &str, show_progress: bool) -> Result<(), String> {
    let dest = dir.join(&m.file);
    let part = dir.join(format!("{}.part", m.file));
    let meta4 = dir.join(format!("{}.meta4", m.file));
    let err = |e: std::io::Error| format!("{}: {e}", part.display());

    let mut f = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&part)
        .map_err(err)?;
    if f.try_lock().is_err() {
        return Err(format!("{} is already being downloaded", m.file));
    }
    fs::write(&meta4, meta4_xml).map_err(|e| format!("{}: {e}", meta4.display()))?;

    // Resuming: the hash has to cover the bytes already on disk.
    let mut hasher = Sha256::new();
    let mut have = f.metadata().map_err(err)?.len();
    if have > m.size {
        f.set_len(0).map_err(err)?;
        have = 0;
    }
    f.seek(std::io::SeekFrom::Start(0)).map_err(err)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut reader = (&f).take(have);
    loop {
        let n = reader.read(&mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let bar = show_progress.then(|| {
        let b = indicatif::ProgressBar::new(m.size).with_position(have);
        b.set_style(
            indicatif::ProgressStyle::with_template("{msg} {wide_bar} {binary_bytes}/{binary_total_bytes} {binary_bytes_per_sec} eta {eta}")
                .unwrap(),
        );
        b.set_message(m.file.clone());
        b
    });

    let mut last_err = String::new();
    // ponytail: mirrors are tried in turn; a stall without an error waits on the OS TCP timeout.
    'mirrors: for url in &m.urls {
        if have == m.size {
            break;
        }
        let resp = ureq::get(url)
            .header("Range", &format!("bytes={have}-"))
            .call();
        let mut resp = match resp {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("{url}: {e}");
                continue;
            }
        };
        if resp.status() != 206 && have > 0 {
            // Mirror ignored the Range: start over.
            f.set_len(0).map_err(err)?;
            hasher = Sha256::new();
            have = 0;
            if let Some(b) = &bar {
                b.set_position(0);
            }
        }
        let mut body = resp.body_mut().with_config().limit(u64::MAX).reader();
        loop {
            match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let n = n.min((m.size - have) as usize);
                    f.write_all(&buf[..n]).map_err(err)?;
                    hasher.update(&buf[..n]);
                    have += n as u64;
                    if let Some(b) = &bar {
                        b.set_position(have);
                    }
                    if have == m.size {
                        break;
                    }
                }
                Err(e) => {
                    last_err = format!("{url}: {e}");
                    continue 'mirrors;
                }
            }
        }
    }
    f.sync_all().map_err(err)?;
    if let Some(b) = &bar {
        b.finish();
    }
    if have < m.size {
        return Err(format!(
            "download incomplete ({} of {}); run the same command again to resume. Last error: {last_err}",
            human_size(have),
            human_size(m.size)
        ));
    }
    let got = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if got != m.sha256 {
        drop(f);
        let _ = fs::remove_file(&part);
        let _ = fs::remove_file(&meta4);
        return Err(format!(
            "checksum mismatch for {} (expected {}, got {got}); the damaged download was deleted",
            m.file, m.sha256
        ));
    }
    drop(f);
    fs::rename(&part, &dest).map_err(err)?;
    let _ = fs::remove_file(&meta4);
    Ok(())
}

/// Downloads in progress, read straight from the folder.
#[derive(Debug, Serialize)]
pub struct Progress {
    pub file: String,
    pub downloaded: u64,
    pub size: u64,
    /// Last line of the background download's log, if it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn status(dir: &Path) -> Result<Vec<Progress>, String> {
    let mut out = vec![];
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = e.map_err(|e| e.to_string())?.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let Some(file) = name
            .strip_suffix(".part")
            .or_else(|| name.strip_suffix(".log"))
        else {
            continue;
        };
        if out.iter().any(|p: &Progress| p.file == file) {
            continue;
        }
        let part = dir.join(format!("{file}.part"));
        let size = fs::read_to_string(dir.join(format!("{file}.meta4")))
            .ok()
            .and_then(|x| catalog::parse_meta4(&x).ok())
            .map_or(0, |m| m.size);
        let error = fs::read_to_string(dir.join(format!("{file}.log")))
            .ok()
            .and_then(|l| {
                l.lines()
                    .rev()
                    .find(|l| l.contains("error"))
                    .map(String::from)
            });
        out.push(Progress {
            file: file.to_string(),
            downloaded: part.metadata().map_or(0, |m| m.len()),
            size,
            error,
        });
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}

pub fn status_md(list: &[Progress]) -> String {
    if list.is_empty() {
        return "No downloads in progress.".into();
    }
    list.iter()
        .map(|p| {
            let pct = if p.size > 0 {
                p.downloaded as f64 * 100.0 / p.size as f64
            } else {
                0.0
            };
            let mut s = format!(
                "- {}: {} of {} ({pct:.1}%)",
                p.file,
                human_size(p.downloaded),
                human_size(p.size)
            );
            if let Some(e) = &p.error {
                s += &format!(" — {e}");
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Start `zim-mcp library add <meta4> --replace` as a detached process that
/// outlives this one, logging to `<file>.log` in the archive folder (removed
/// when it succeeds). Same code path as the CLI.
pub fn spawn_background(dir: &Path, plan: &Plan) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let log_path = dir.join(format!("{}.log", plan.file));
    let log = File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["library", "add", &plan.meta4_url, "--replace", "--log"])
        .arg("--dir")
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // not killed with the MCP client's process group
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0000_0200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    cmd.spawn()
        .map_err(|e| format!("can't start download: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_describes_replacement() {
        let p = Plan {
            name: "wikipedia_en_all_maxi".into(),
            file: "wikipedia_en_all_maxi_2026-08.zim".into(),
            size: 119 * 1024 * 1024 * 1024,
            meta4_url: String::new(),
            replaces: vec!["wikipedia_en_all_maxi_2026-02.zim".into()],
            already_present: false,
        };
        assert_eq!(
            p.describe(Path::new("zim")),
            "Download wikipedia_en_all_maxi_2026-08.zim (119.0 GiB) to zim? This replaces wikipedia_en_all_maxi_2026-02.zim."
        );
        assert_eq!(
            meta4_url("https://x.org/a_2026-01.zim").unwrap(),
            "https://x.org/a_2026-01.zim.meta4"
        );
    }
}
