//! `Archive`: open a ZIM file and look entries up, the way libzim does
//! (`fileimpl.cpp`, `archive.cpp`, `dirent_lookup.h`).

use crate::file::Parts;
use crate::format::{Cluster, Compression, Dirent, HEADER_SIZE, Header, Kind};
use crate::{Error, Result};
use md5::{Digest, Md5};
use std::cmp::Ordering;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

const CACHE_CLUSTERS: usize = 16;
const WATCHDOG: u32 = 50;

/// One entry (path-ordered `index`). Strings are lossy UTF-8 for display;
/// lookups compare the raw bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub index: u32,
    /// In old-namespace archives this is the long path (`A/Foo`), as libzim reports it.
    pub path: String,
    /// The title, or the path when the title is empty.
    pub title: String,
    /// Empty for redirects.
    pub mimetype: String,
    pub redirect: Option<u32>,
    pub(crate) cluster: u32,
    pub(crate) blob: u32,
}

impl Entry {
    pub fn is_redirect(&self) -> bool {
        self.redirect.is_some()
    }
}

pub struct Archive {
    file: Parts,
    header: Header,
    mimes: Vec<String>,
    /// User entries: the `C` range, or everything in old-namespace archives.
    user: (u32, u32),
    /// Title index: file offset and length (u32 entry indexes), and whether
    /// it's the v1 front-article listing.
    titles: (u64, u32, bool),
    // ponytail: up to 16 whole decompressed clusters, memory ≈ 16 × cluster size
    // (2 MiB by default); stream blobs like libzim if that ever hurts.
    cache: Mutex<Vec<(u32, Arc<Cluster>)>>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Archive> {
        let file = Parts::open(path)?;
        if file.size < HEADER_SIZE {
            return Err(Error::Format(
                "zim-file is too small to contain a header".into(),
            ));
        }
        let header = Header::parse(&file.read(0, HEADER_SIZE)?)?;
        if header.has_checksum() && header.checksum_pos.checked_add(16) != Some(file.size) {
            return Err(Error::Format(
                "Zim file(s) is of bad size or corrupted.".into(),
            ));
        }
        let n = header.entry_count as u64;
        let size = file.size;
        let in_file = |what: &str, off: u64, len: u64| {
            if off.checked_add(len).is_none_or(|e| e > size) {
                Err(Error::Format(format!(
                    "{what} outside (or not fully inside) ZIM file."
                )))
            } else {
                Ok(())
            }
        };
        in_file("Dirent pointer table", header.path_ptr_pos, 8 * n)?;
        in_file(
            "Cluster pointer table",
            header.cluster_ptr_pos,
            8 * header.cluster_count as u64,
        )?;
        let mut a = Archive {
            file,
            header,
            mimes: vec![],
            user: (0, 0),
            titles: (0, 0, false),
            cache: Mutex::new(vec![]),
        };
        if a.header.cluster_count > 0 {
            let last = a.cluster_offset(a.header.cluster_count - 1)?;
            if last > a.file.size {
                return Err(Error::Format(
                    "last cluster offset larger than file size; file corrupt".into(),
                ));
            }
        }
        a.user = if a.header.new_namespaces() {
            (a.ns_begin(b'C')?, a.ns_begin(b'D')?)
        } else {
            (0, a.header.entry_count)
        };
        a.titles = match a.title_listing_v1()? {
            Some(t) => t,
            None if a.header.has_title_v0() => {
                in_file("Title index table", a.header.title_idx_pos, 4 * n)?;
                (a.header.title_idx_pos, a.header.entry_count, false)
            }
            None => {
                return Err(Error::Format(
                    "Zim file doesn't contain a title ordered index".into(),
                ));
            }
        };
        a.mimes = a.read_mimes()?;
        Ok(a)
    }

    /// `X/listing/titleOrdered/v1`, if present in an uncompressed cluster.
    fn title_listing_v1(&self) -> Result<Option<(u64, u32, bool)>> {
        let Some(i) = self.find(b'X', b"listing/titleOrdered/v1")? else {
            return Ok(None);
        };
        let Kind::Item { cluster, blob, .. } = self.dirent(i)?.kind else {
            return Ok(None);
        };
        let c = self.cluster(cluster)?;
        if c.compression != Compression::None {
            return Ok(None); // a format error, tolerated as libzim does
        }
        if blob >= c.count() {
            return Err(Error::Format("blob index out of range".into()));
        }
        let b = blob as usize;
        let off = self.cluster_offset(cluster)? + 1 + c.offsets[b];
        let len = c.offsets[b + 1] - c.offsets[b];
        if off.checked_add(len).is_none_or(|e| e > self.file.size) {
            return Err(Error::Format(
                "Title index v1 outside (or not fully inside) ZIM file.".into(),
            ));
        }
        Ok(Some((off, (len / 4) as u32, true)))
    }

    /// `FileImpl::readMimeTypes`.
    fn read_mimes(&self) -> Result<Vec<String>> {
        let h = &self.header;
        let mut end = h.path_ptr_pos.min(h.cluster_ptr_pos);
        if h.has_title_v0() {
            end = end.min(h.title_idx_pos);
        }
        if h.entry_count > 0 {
            end = end.min(self.file.u64_at(h.path_ptr_pos)?);
            end = end.min(self.cluster_offset(0)?);
        }
        if end <= h.mime_list_pos {
            return Err(Error::Format("Bad ZIM archive".into()));
        }
        let buf = self.file.read(h.mime_list_pos, end - h.mime_list_pos)?;
        let mut out = vec![];
        let mut p = 0;
        while buf[p] != 0 {
            let n = buf[p..]
                .iter()
                .position(|&c| c == 0)
                .ok_or_else(|| Error::Format("Error getting mimelists.".into()))?;
            out.push(String::from_utf8_lossy(&buf[p..p + n]).into_owned());
            p += n + 1;
            if p >= buf.len() {
                return Err(Error::Format("Error getting mimelists.".into()));
            }
        }
        Ok(out)
    }

    fn cluster_offset(&self, i: u32) -> Result<u64> {
        self.file.u64_at(self.header.cluster_ptr_pos + 8 * i as u64)
    }

    fn cluster(&self, i: u32) -> Result<Arc<Cluster>> {
        if i >= self.header.cluster_count {
            return Err(Error::Format("cluster index out of range".into()));
        }
        let mut cache = self.cache.lock().unwrap();
        if let Some(p) = cache.iter().position(|(k, _)| *k == i) {
            let hit = cache.remove(p);
            cache.push(hit.clone());
            return Ok(hit.1);
        }
        drop(cache); // don't hold the lock while decompressing
        let off = self.cluster_offset(i)?;
        let c = Arc::new(Cluster::read(
            self.file.reader(off),
            self.header.entry_count as u64,
        )?);
        let mut cache = self.cache.lock().unwrap();
        if cache.len() >= CACHE_CLUSTERS {
            cache.remove(0);
        }
        cache.push((i, c.clone()));
        Ok(c)
    }

    /// The dirent at path-ordered index `i`.
    fn dirent(&self, i: u32) -> Result<Dirent> {
        if i >= self.header.entry_count {
            return Err(Error::Format("entry index out of range".into()));
        }
        let off = self.file.u64_at(self.header.path_ptr_pos + 8 * i as u64)?;
        if off >= self.file.size {
            return Err(Error::Format("Invalid dirent pointer".into()));
        }
        // Size unknown up front: read 256 bytes, retry with 256 more.
        let mut len = 256u64.min(self.file.size - off);
        loop {
            if let Some(d) = Dirent::parse(&self.file.read(off, len)?)? {
                return Ok(d);
            }
            if off + len >= self.file.size {
                return Err(Error::Format("truncated dirent".into()));
            }
            len = (len + 256).min(self.file.size - off);
        }
    }

    fn cmp_at(&self, ns: u8, key: &[u8], i: u32, by_title: bool) -> Result<Ordering> {
        let d = if by_title {
            self.dirent(self.title_at(i)?)?
        } else {
            self.dirent(i)?
        };
        let k = if by_title { d.title() } else { &d.path };
        Ok(ns.cmp(&d.ns).then_with(|| key.cmp(k)))
    }

    /// `DirentLookup::findInRange` over `0..count`: `(found, index)`, where a
    /// miss gives the insertion point.
    fn search(&self, ns: u8, key: &[u8], count: u32, by_title: bool) -> Result<(bool, u32)> {
        if count == 0 {
            return Ok((false, 0));
        }
        match self.cmp_at(ns, key, 0, by_title)? {
            Ordering::Less => return Ok((false, 0)),
            Ordering::Equal => return Ok((true, 0)),
            Ordering::Greater => {}
        }
        if self.cmp_at(ns, key, count - 1, by_title)? == Ordering::Greater {
            return Ok((false, count));
        }
        // (at l) < key <= (at u)
        let (mut l, mut u) = (0, count - 1);
        loop {
            let p = l + (u - l).div_ceil(2);
            let c = self.cmp_at(ns, key, p, by_title)?;
            if c != Ordering::Greater {
                if u == p {
                    return Ok((c == Ordering::Equal, u));
                }
                u = p;
            } else {
                l = p;
            }
        }
    }

    fn find(&self, ns: u8, path: &[u8]) -> Result<Option<u32>> {
        let (found, i) = self.search(ns, path, self.header.entry_count, false)?;
        Ok(found.then_some(i))
    }

    /// First path-ordered index whose namespace is >= `ns`.
    fn ns_begin(&self, ns: u8) -> Result<u32> {
        let n = self.header.entry_count;
        if n == 0 {
            return Ok(0);
        }
        let (mut lo, mut hi) = (0, n);
        while hi - lo > 1 {
            let m = lo + (hi - lo) / 2;
            if self.dirent(m)?.ns >= ns {
                hi = m;
            } else {
                lo = m;
            }
        }
        Ok(if self.dirent(0)?.ns < ns { hi } else { lo })
    }

    fn entry(&self, index: u32, d: Dirent) -> Result<Entry> {
        let short = String::from_utf8_lossy(&d.path).into_owned();
        let path = if self.header.new_namespaces() {
            short
        } else {
            format!("{}/{short}", d.ns as char)
        };
        let title = String::from_utf8_lossy(d.title()).into_owned();
        let (mimetype, redirect, cluster, blob) = match d.kind {
            Kind::Redirect(t) => (String::new(), Some(t), 0, 0),
            Kind::Item {
                mime,
                cluster,
                blob,
            } => (self.mime(mime)?, None, cluster, blob),
            Kind::Other(m) => (self.mime(m)?, None, 0, 0),
        };
        Ok(Entry {
            index,
            path,
            title,
            mimetype,
            redirect,
            cluster,
            blob,
        })
    }

    fn mime(&self, i: u16) -> Result<String> {
        self.mimes
            .get(i as usize)
            .cloned()
            .ok_or_else(|| Error::Format(format!("unknown mime type code {i}")))
    }

    pub fn uuid(&self) -> [u8; 16] {
        self.header.uuid
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Every entry, including metadata and indexes.
    pub fn all_entry_count(&self) -> u32 {
        self.header.entry_count
    }

    /// User entries: the `C` namespace (or everything in old archives).
    pub fn entry_count(&self) -> u32 {
        self.user.1 - self.user.0
    }

    /// Entry at absolute path-ordered index `index`.
    pub fn entry_at(&self, index: u32) -> Result<Entry> {
        let d = self.dirent(index)?;
        self.entry(index, d)
    }

    /// libzim's `getEntryByPath`: `C/<path>` (also `X/<path>` with the
    /// namespace stripped); in old archives the long path, then `A`, `I`, `J`, `-`.
    pub fn entry_by_path(&self, path: &str) -> Option<Entry> {
        let p = path.as_bytes();
        let i = if self.header.new_namespaces() {
            self.find(b'C', p).ok().flatten().or_else(|| {
                let (_, short) = parse_long_path(p)?;
                self.find(b'C', short).ok().flatten()
            })
        } else {
            parse_long_path(p)
                .and_then(|(ns, short)| self.find(ns, short).ok().flatten())
                .or_else(|| {
                    [b'A', b'I', b'J', b'-']
                        .iter()
                        .find_map(|&ns| self.find(ns, p).ok().flatten())
                })
        }?;
        self.entry_at(i).ok()
    }

    /// `W/mainPage` (itself a redirect), else the header's main page.
    pub fn main_entry(&self) -> Option<Entry> {
        if let Ok(Some(i)) = self.find(b'W', b"mainPage") {
            return self.entry_at(i).ok();
        }
        match self.header.main_page {
            u32::MAX => None,
            i => self.entry_at(i).ok(),
        }
    }

    /// Follow redirects (at most 50 hops) to the entry holding the content.
    pub fn resolve(&self, e: &Entry) -> Result<Entry> {
        let mut e = e.clone();
        for _ in 0..WATCHDOG {
            match e.redirect {
                Some(t) => e = self.entry_at(t)?,
                None => return Ok(e),
            }
        }
        Err(Error::Format(format!("{} is a redirect loop", e.path)))
    }

    /// The content of `e`, following redirects.
    pub fn content(&self, e: &Entry) -> Result<Vec<u8>> {
        let e = self.resolve(e)?;
        let c = self.cluster(e.cluster)?;
        Ok(c.blob(e.blob).to_vec())
    }

    /// `M/<name>`, following redirects.
    pub fn metadata(&self, name: &str) -> Option<Vec<u8>> {
        let mut i = self.find(b'M', name.as_bytes()).ok()??;
        let mut e = self.entry_at(i).ok()?;
        for _ in 1..WATCHDOG {
            let Some(t) = e.redirect else { break };
            i = t;
            e = self.entry_at(i).ok()?;
        }
        if e.is_redirect() {
            return None;
        }
        self.content(&e).ok()
    }

    /// Front articles (with the v1 listing), else the `M/Counter` `text/html*`
    /// total, else the size of the `C` (or `A`) namespace.
    pub fn article_count(&self) -> u32 {
        if self.titles.2 {
            return self.titles.1;
        }
        if let Some(c) = self.metadata("Counter") {
            return parse_counter(&String::from_utf8_lossy(&c))
                .iter()
                .filter(|(m, _)| m.starts_with("text/html"))
                .map(|(_, n)| n)
                .sum();
        }
        let ns = if self.header.new_namespaces() {
            b'C'
        } else {
            b'A'
        };
        let n = || Ok::<_, Error>(self.ns_begin(ns + 1)? - self.ns_begin(ns)?);
        n().unwrap_or(0)
    }

    /// Path-ordered index of title-ordered position `i`.
    fn title_at(&self, i: u32) -> Result<u32> {
        if i >= self.titles.1 {
            return Err(Error::Format("entry index out of range".into()));
        }
        self.file.u32_at(self.titles.0 + 4 * i as u64)
    }

    /// Whether the title index is the v1 front-article listing.
    pub fn has_front_articles_index(&self) -> bool {
        self.titles.2
    }

    /// Entry indexes in title order: the front articles (v1 listing), or every
    /// entry (v0 index).
    pub fn front_articles(&self) -> Result<Vec<u32>> {
        let b = self.file.read(self.titles.0, 4 * self.titles.1 as u64)?;
        Ok(b.chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect())
    }

    /// Title suggestions without a Xapian index (`Archive::findByTitle`): the
    /// entries in `C` (`A` in old archives) whose title starts with `query`,
    /// compared byte for byte, in title order.
    pub fn suggest(&self, query: &str, limit: usize) -> Vec<Entry> {
        let q = query.as_bytes();
        let Some(&last) = q.last() else {
            return vec![];
        };
        let ns = if self.header.new_namespaces() {
            b'C'
        } else {
            b'A'
        };
        let mut next = q.to_vec();
        *next.last_mut().unwrap() = last.wrapping_add(1);
        let n = self.titles.1;
        let (Ok((_, begin)), Ok((_, end))) =
            (self.search(ns, q, n, true), self.search(ns, &next, n, true))
        else {
            return vec![];
        };
        (begin..end.max(begin))
            .take(limit)
            .filter_map(|t| self.entry_at(self.title_at(t).ok()?).ok())
            .collect()
    }

    /// MD5 of everything before the checksum. `Ok(false)` if there's no checksum.
    pub fn verify_checksum(&self) -> Result<bool> {
        if !self.header.has_checksum() {
            return Ok(false);
        }
        let pos = self.header.checksum_pos;
        let mut md5 = Md5::new();
        let mut r = self.file.reader(0).take(pos);
        let mut buf = vec![0; 1 << 20];
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                break;
            }
            md5.update(&buf[..n]);
        }
        Ok(md5.finalize().as_slice() == self.file.read(pos, 16)?)
    }
}

/// `parseLongPath`: `[/]N/rest` → (N, rest).
fn parse_long_path(p: &[u8]) -> Option<(u8, &[u8])> {
    let i = usize::from(p.first() == Some(&b'/'));
    if i + 1 > p.len() || p[i] == b'/' || (i + 1 < p.len() && p[i + 1] != b'/') {
        return None;
    }
    Some((p[i], &p[(i + 2).min(p.len())..]))
}

/// `M/Counter`: `mime=count;mime=count`, where a mimetype may carry
/// `;key=value` parameters (libzim's `parseMimetypeCounter`).
pub(crate) fn parse_counter(s: &str) -> Vec<(String, u32)> {
    let mut out = vec![];
    let mut parts = s.split(';');
    while let Some(first) = parts.next() {
        let mut item = first.to_string();
        if !item.contains('=') {
            let mut complete = false;
            for p in parts.by_ref() {
                item += ";";
                item += p;
                if p.matches('=').count() == 2 {
                    complete = true;
                    break;
                }
            }
            if !complete {
                break;
            }
        }
        if let Some((m, n)) = item.rsplit_once('=')
            && let Ok(n) = n.parse()
            && !m.is_empty()
        {
            out.push((m.to_string(), n));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_paths() {
        assert_eq!(parse_long_path(b"A/foo"), Some((b'A', &b"foo"[..])));
        assert_eq!(parse_long_path(b"/A/foo"), Some((b'A', &b"foo"[..])));
        assert_eq!(parse_long_path(b"A"), Some((b'A', &b""[..])));
        assert_eq!(parse_long_path(b"AB/x"), None);
        assert_eq!(parse_long_path(b"/"), None);
        assert_eq!(parse_long_path(b""), None);
    }

    #[test]
    fn counter() {
        assert_eq!(
            parse_counter("text/html=3;image/png=2;text/html;raw=true=4"),
            vec![
                ("text/html".into(), 3),
                ("image/png".into(), 2),
                ("text/html;raw=true".into(), 4)
            ]
        );
        assert_eq!(parse_counter(""), vec![]);
        assert_eq!(parse_counter("a=x;b=1"), vec![("b".into(), 1)]);
    }
}
