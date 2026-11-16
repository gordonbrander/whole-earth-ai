//! Positional reads over one file or a split archive (`foo.zimaa`, `foo.zimab`, …),
//! which concatenate into one logical file.

use crate::{Error, Result};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub(crate) struct Part {
    file: File,
    pub path: PathBuf,
    /// Where the part starts in `file` (non-zero for an embedded archive).
    offset: u64,
    /// Where the part starts in the logical file.
    start: u64,
    len: u64,
}

pub(crate) struct Parts {
    parts: Vec<Part>,
    pub size: u64,
    /// libzim's `getFilename`: the path as opened, or `<base>aa` for a split archive.
    pub path: PathBuf,
}

#[cfg(unix)]
fn read_exact_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(f, buf, off)
}

#[cfg(windows)]
fn read_exact_at(f: &File, mut buf: &mut [u8], mut off: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match f.seek_read(buf, off) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                buf = &mut buf[n..];
                off += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

impl Parts {
    /// `path` itself, or else its split parts `<path>aa`, `<path>ab`, … (libzim's
    /// `openSinglePieceOrSplitZimFile`; a path ending in `.zimaa` asks for the split).
    pub fn open(path: &Path) -> Result<Parts> {
        let s = path.as_os_str().to_string_lossy();
        if !s.ends_with(".zimaa")
            && let Ok(f) = File::open(path)
        {
            let len = f.metadata()?.len();
            return Ok(Parts::new(vec![(f, path.into(), 0, len)]));
        }
        let base = s.strip_suffix("aa").filter(|_| s.ends_with(".zimaa"));
        let base = base.unwrap_or(&s).to_string();
        let mut parts = vec![];
        'outer: for a in b'a'..=b'z' {
            for b in b'a'..=b'z' {
                let name = PathBuf::from(format!("{base}{}{}", a as char, b as char));
                let Ok(f) = File::open(&name) else {
                    break 'outer;
                };
                let len = f.metadata()?.len();
                parts.push((f, name, 0, len));
            }
        }
        if parts.is_empty() {
            let split = if s.ends_with(".zimaa") {
                "as a split "
            } else {
                ""
            };
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Error opening {split}ZIM file: {}", path.display()),
            )));
        }
        let mut p = Parts::new(parts);
        p.path = format!("{base}aa").into();
        Ok(p)
    }

    /// Byte ranges `(file, offset, len)` that concatenate into one archive.
    pub fn open_ranges(ranges: &[(PathBuf, u64, u64)]) -> Result<Parts> {
        let mut parts = vec![];
        for (path, offset, len) in ranges {
            let f = File::open(path)?;
            let flen = f.metadata()?.len();
            if offset.checked_add(*len).is_none_or(|end| end > flen) {
                return Err(Error::Format(format!(
                    "{}: range {offset} (+{len}) is past the end of the file",
                    path.display()
                )));
            }
            parts.push((f, path.clone(), *offset, *len));
        }
        if parts.is_empty() {
            return Err(Error::Format("no parts given".into()));
        }
        Ok(Parts::new(parts))
    }

    fn new(ranges: Vec<(File, PathBuf, u64, u64)>) -> Parts {
        let path = ranges[0].1.clone();
        let mut size = 0;
        let parts = ranges
            .into_iter()
            .map(|(file, path, offset, len)| {
                let start = size;
                size += len;
                Part {
                    file,
                    path,
                    offset,
                    start,
                    len,
                }
            })
            .collect();
        Parts { parts, size, path }
    }

    pub fn is_multipart(&self) -> bool {
        self.parts.len() > 1
    }

    /// The part holding logical bytes `off..off + len`, and the physical
    /// offset there; `None` if they span parts.
    pub fn locate(&self, off: u64, len: u64) -> Option<(&Path, u64)> {
        let p = self
            .parts
            .get(self.parts.partition_point(|p| p.start + p.len <= off))?;
        (off + len <= p.start + p.len).then(|| (p.path.as_path(), p.offset + off - p.start))
    }

    pub fn read_into(&self, mut buf: &mut [u8], mut off: u64) -> Result<()> {
        if off
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > self.size)
        {
            return Err(Error::Format(format!(
                "read of {} bytes at {off} is past the end of the file",
                buf.len()
            )));
        }
        // First part that ends after `off`.
        let mut i = self.parts.partition_point(|p| p.start + p.len <= off);
        while !buf.is_empty() {
            let p = &self.parts[i];
            let n = buf.len().min((p.start + p.len - off) as usize);
            read_exact_at(&p.file, &mut buf[..n], p.offset + off - p.start)?;
            buf = &mut buf[n..];
            off += n as u64;
            i += 1;
        }
        Ok(())
    }

    /// `len` bytes at `off`, checked against the file size before allocating.
    pub fn read(&self, off: u64, len: u64) -> Result<Vec<u8>> {
        if off.checked_add(len).is_none_or(|end| end > self.size) {
            return Err(Error::Format(format!(
                "section at {off} (+{len}) is outside the file"
            )));
        }
        let mut v = vec![0; len as usize];
        self.read_into(&mut v, off)?;
        Ok(v)
    }

    pub fn u32_at(&self, off: u64) -> Result<u32> {
        let mut b = [0; 4];
        self.read_into(&mut b, off)?;
        Ok(u32::from_le_bytes(b))
    }

    pub fn u64_at(&self, off: u64) -> Result<u64> {
        let mut b = [0; 8];
        self.read_into(&mut b, off)?;
        Ok(u64::from_le_bytes(b))
    }

    /// A sequential reader from `off` to the end of the file.
    pub fn reader(&self, off: u64) -> At<'_> {
        At {
            parts: self,
            pos: off,
        }
    }
}

pub(crate) struct At<'a> {
    parts: &'a Parts,
    pos: u64,
}

impl Read for At<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf
            .len()
            .min(self.parts.size.saturating_sub(self.pos) as usize);
        self.parts
            .read_into(&mut buf[..n], self.pos)
            .map_err(io::Error::other)?;
        self.pos += n as u64;
        Ok(n)
    }
}
