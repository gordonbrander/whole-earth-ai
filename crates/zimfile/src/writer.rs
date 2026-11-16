//! `Creator`: write a ZIM archive that libzim reads. A port of libzim's
//! `writer/creator.cpp` (layout, orderings, counter, title listing, redirect
//! clean-up), single-threaded.
//!
//! Layout: header (written last) · mime list at 80 · clusters from 2048 ·
//! dirents in path order · path pointers · cluster pointers · MD5.

use crate::format::{Compression, DELETED, Dirent, HEADER_SIZE, Header, Kind};
use crate::{Error, Result};
use md5::{Digest, Md5};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const CLUSTER_BASE_OFFSET: u64 = 2048;
const DEFAULT_CLUSTER_SIZE: usize = 2 << 20;
const ZSTD_LEVEL: i32 = 19; // libzim's
const LZMA_PRESET: u32 = 9;

/// One content entry for [`Creator::add_item`]. `front` and `compress` are
/// libzim's hints; `None` means the default from the mimetype.
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub path: String,
    pub title: String,
    pub mimetype: String,
    pub content: Vec<u8>,
    /// A front article: listed by title, counted in `article_count`. Default: HTML.
    pub front: Option<bool>,
    /// Put in a compressed cluster. Default: text-like mimetypes.
    pub compress: Option<bool>,
}

impl Item {
    pub fn new(path: &str, title: &str, mimetype: &str, content: impl Into<Vec<u8>>) -> Item {
        Item {
            path: path.into(),
            title: title.into(),
            mimetype: mimetype.into(),
            content: content.into(),
            ..Item::default()
        }
    }
}

/// libzim's `isCompressibleMimetype`.
fn compressible(mime: &str) -> bool {
    mime.starts_with("text")
        || mime.contains("+xml")
        || mime.contains("+json")
        || mime == "application/javascript"
        || mime == "application/json"
}

#[derive(Debug, Clone)]
enum Target {
    /// Content: (registration mime index, cluster serial, blob).
    Item(u16, u32, u32),
    /// A redirect to (namespace, path).
    Redirect(u8, Vec<u8>),
    /// Named as a redirect target but not added (yet).
    Placeholder,
}

#[derive(Debug, Clone)]
struct WDirent {
    title: Vec<u8>,
    target: Target,
    front: bool,
}

/// A cluster being filled.
struct Open {
    serial: u32,
    data: Vec<u8>,
    ends: Vec<u64>,
}

impl Open {
    fn size(&self) -> u64 {
        // libzim's estimate: offset table (incl. the leading 0) + data.
        4 * (self.ends.len() as u64 + 1) + self.data.len() as u64
    }
}

pub struct Creator {
    path: PathBuf,
    tmp: PathBuf,
    compression: Compression,
    cluster_size: usize,
    main_path: Option<String>,
    uuid: Option<[u8; 16]>,
    out: Option<BufWriter<File>>,
    dirents: BTreeMap<(u8, Vec<u8>), WDirent>,
    mimes: Vec<String>,
    mime_idx: HashMap<String, u16>,
    comp: Open,
    uncomp: Open,
    next_serial: u32,
    /// Closed clusters: serial → (index, file offset).
    closed: HashMap<u32, (u32, u64)>,
    offsets: Vec<u64>,
    counter: BTreeMap<String, u32>,
}

impl Creator {
    /// A creator writing to `path` (through `<path>.tmp`): zstd, 2 MiB clusters.
    pub fn new(path: &Path) -> Creator {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        Creator {
            path: path.into(),
            tmp: tmp.into(),
            compression: Compression::Zstd,
            cluster_size: DEFAULT_CLUSTER_SIZE,
            main_path: None,
            uuid: None,
            out: None,
            dirents: BTreeMap::new(),
            mimes: vec![],
            mime_idx: HashMap::new(),
            comp: Open {
                serial: 0,
                data: vec![],
                ends: vec![],
            },
            uncomp: Open {
                serial: 1,
                data: vec![],
                ends: vec![],
            },
            next_serial: 2,
            closed: HashMap::new(),
            offsets: vec![],
            counter: BTreeMap::new(),
        }
    }

    pub fn compression(mut self, c: Compression) -> Self {
        self.compression = c;
        self
    }
    pub fn cluster_size(mut self, n: usize) -> Self {
        self.cluster_size = n;
        self
    }
    /// The main page: `W/mainPage` redirects to `C/<p>`.
    pub fn main_path(mut self, p: &str) -> Self {
        self.main_path = Some(p.into());
        self
    }
    pub fn uuid(mut self, u: [u8; 16]) -> Self {
        self.uuid = Some(u);
        self
    }

    fn mime(&mut self, m: &str) -> Result<u16> {
        if let Some(&i) = self.mime_idx.get(m) {
            return Ok(i);
        }
        let i = u16::try_from(self.mimes.len())
            .ok()
            .filter(|&i| i < DELETED)
            .ok_or_else(|| Error::Invalid("too many distinct mime types".into()))?;
        self.mimes.push(m.into());
        self.mime_idx.insert(m.into(), i);
        Ok(i)
    }

    /// `ensureDirentCanBeAdded`: only a placeholder may be replaced.
    fn check_new(&self, ns: u8, path: &str, title: &str) -> Result<()> {
        if path.len() + title.len() + 1 > u16::MAX as usize {
            return Err(Error::Invalid(format!(
                "{path}: path and title are too long"
            )));
        }
        match self.dirents.get(&(ns, path.as_bytes().to_vec())) {
            Some(d) if !matches!(d.target, Target::Placeholder) => Err(Error::Invalid(format!(
                "Impossible to add {}/{path}: it already exists",
                ns as char
            ))),
            _ => Ok(()),
        }
    }

    fn out(&mut self) -> Result<&mut BufWriter<File>> {
        if self.out.is_none() {
            let file = File::options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&self.tmp)?;
            let mut f = BufWriter::new(file);
            f.seek(SeekFrom::Start(CLUSTER_BASE_OFFSET))?;
            self.out = Some(f);
        }
        Ok(self.out.as_mut().unwrap())
    }

    /// Add a blob to the compressed or uncompressed cluster, closing it first
    /// if this would overflow it. Returns (cluster serial, blob index).
    fn add_data(&mut self, content: &[u8], compress: bool) -> Result<(u32, u32)> {
        let open = if compress { &self.comp } else { &self.uncomp };
        if !open.ends.is_empty() && open.size() + content.len() as u64 >= self.cluster_size as u64 {
            self.close_cluster(compress)?;
        }
        let open = if compress {
            &mut self.comp
        } else {
            &mut self.uncomp
        };
        open.data.extend_from_slice(content);
        open.ends.push(open.data.len() as u64);
        Ok((open.serial, open.ends.len() as u32 - 1))
    }

    // ponytail: compresses inline on one core (zstd level 19, as libzim); move
    // cluster compression onto scoped threads when big builds matter.
    fn close_cluster(&mut self, compress: bool) -> Result<()> {
        let fresh = Open {
            serial: self.next_serial,
            data: vec![],
            ends: vec![],
        };
        self.next_serial += 1;
        let c = std::mem::replace(
            if compress {
                &mut self.comp
            } else {
                &mut self.uncomp
            },
            fresh,
        );
        let extended = c.data.len() as u64 > u32::MAX as u64;
        let w: u64 = if extended { 8 } else { 4 };
        let table = w * (c.ends.len() as u64 + 1);
        let mut raw = Vec::with_capacity(table as usize + c.data.len());
        for off in std::iter::once(0).chain(c.ends.iter().copied()) {
            if extended {
                raw.extend((off + table).to_le_bytes());
            } else {
                raw.extend(((off + table) as u32).to_le_bytes());
            }
        }
        raw.extend_from_slice(&c.data);
        drop(c.data);
        let comp = if compress {
            self.compression
        } else {
            Compression::None
        };
        let (code, body) = match comp {
            Compression::None => (1u8, raw),
            Compression::Zstd => (5, zstd::encode_all(&raw[..], ZSTD_LEVEL)?),
            Compression::Lzma => {
                let mut v = vec![];
                liblzma::read::XzEncoder::new(&raw[..], LZMA_PRESET).read_to_end(&mut v)?;
                (4, v)
            }
        };
        let f = self.out()?;
        let offset = f.stream_position()?;
        f.write_all(&[code | if extended { 0x10 } else { 0 }])?;
        f.write_all(&body)?;
        let index = self.offsets.len() as u32;
        self.offsets.push(offset);
        self.closed.insert(c.serial, (index, offset));
        Ok(())
    }

    fn put(&mut self, ns: u8, path: &str, d: WDirent) {
        self.dirents.insert((ns, path.as_bytes().to_vec()), d);
    }

    /// Add user content (`C/<path>`).
    pub fn add_item(&mut self, item: Item) -> Result<()> {
        let mime = if item.mimetype.is_empty() {
            "application/octet-stream".to_string()
        } else {
            item.mimetype
        };
        self.check_new(b'C', &item.path, &item.title)?;
        let front = item.front.unwrap_or(mime.starts_with("text/html"));
        let compress = item.compress.unwrap_or(compressible(&mime));
        let m = self.mime(&mime)?;
        let (cluster, blob) = self.add_data(&item.content, compress)?;
        let clean = &mime[..mime.find([';', ' ', '\t']).unwrap_or(mime.len())];
        if !clean.is_empty() {
            *self.counter.entry(clean.to_string()).or_default() += 1;
        }
        let title = title_bytes(&item.path, &item.title);
        let target = Target::Item(m, cluster, blob);
        self.put(
            b'C',
            &item.path,
            WDirent {
                title,
                target,
                front,
            },
        );
        Ok(())
    }

    /// Add `M/<name>`.
    pub fn add_metadata(&mut self, name: &str, value: &[u8], mimetype: &str) -> Result<()> {
        self.check_new(b'M', name, "")?;
        let m = self.mime(mimetype)?;
        let (cluster, blob) = self.add_data(value, compressible(mimetype))?;
        let target = Target::Item(m, cluster, blob);
        self.put(
            b'M',
            name,
            WDirent {
                title: vec![],
                target,
                front: false,
            },
        );
        Ok(())
    }

    /// Add a square PNG illustration (`M/Illustration_<size>x<size>@1`).
    pub fn add_illustration(&mut self, size: u32, png: &[u8]) -> Result<()> {
        self.add_metadata(&format!("Illustration_{size}x{size}@1"), png, "image/png")
    }

    /// Add `C/<path>` redirecting to `C/<target>` (which may be added later).
    pub fn add_redirection(
        &mut self,
        path: &str,
        title: &str,
        target: &str,
        front: bool,
    ) -> Result<()> {
        let key = (b'C', target.as_bytes().to_vec());
        self.dirents.entry(key).or_insert(WDirent {
            title: vec![],
            target: Target::Placeholder,
            front: false,
        });
        self.check_new(b'C', path, title)?;
        let d = WDirent {
            title: title_bytes(path, title),
            target: Target::Redirect(b'C', target.as_bytes().to_vec()),
            front,
        };
        self.put(b'C', path, d);
        Ok(())
    }

    /// Drop redirects to missing entries, and loops and chains that don't end
    /// at content (`detectDanglingRedirects`, `removeLoopsAndBlindChainsOfRedirects`).
    fn drop_bad_redirects(&mut self) {
        let resolves = |start: &(u8, Vec<u8>)| {
            let mut key = start.clone();
            for _ in 0..=self.dirents.len() {
                match self.dirents.get(&key).map(|d| &d.target) {
                    Some(Target::Item(..)) => return true,
                    Some(Target::Redirect(ns, p)) => key = (*ns, p.clone()),
                    _ => return false,
                }
            }
            false // a loop
        };
        let bad: Vec<_> = self
            .dirents
            .iter()
            .filter(|(k, d)| match d.target {
                Target::Item(..) => false,
                Target::Placeholder => true,
                Target::Redirect(..) => !resolves(k),
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in bad {
            self.dirents.remove(&k);
        }
    }

    /// Sort, index and write everything; then the checksum, and rename into place.
    pub fn finish(mut self) -> Result<()> {
        const LISTING: &[u8] = b"listing/titleOrdered/v1";
        if let Some(main) = self.main_path.clone()
            && self.dirents.contains_key(&(b'C', main.as_bytes().to_vec()))
        {
            let d = WDirent {
                title: vec![],
                target: Target::Redirect(b'C', main.into_bytes()),
                front: false,
            };
            self.put(b'W', "mainPage", d);
        }
        self.drop_bad_redirects();

        // M/Counter, in a compressed cluster (libzim: `mime=n;mime=n`, sorted).
        let counter: Vec<String> = self
            .counter
            .iter()
            .map(|(m, n)| format!("{m}={n}"))
            .collect();
        let counter = counter.join(";");
        self.check_new(b'M', "Counter", "")?;
        let m = self.mime("text/plain")?;
        let (cluster, blob) = self.add_data(counter.as_bytes(), true)?;
        self.put(
            b'M',
            "Counter",
            WDirent {
                title: vec![],
                target: Target::Item(m, cluster, blob),
                front: false,
            },
        );

        // Every dirent exists from here on: indexes are positions in path order.
        let listing_mime = self.mime("application/octet-stream+zimlisting")?;
        let listing_key = (b'X', LISTING.to_vec());
        self.dirents.insert(
            listing_key.clone(),
            WDirent {
                title: vec![],
                target: Target::Placeholder, // filled in below
                front: false,
            },
        );
        let index: HashMap<(u8, Vec<u8>), u32> = self
            .dirents
            .keys()
            .enumerate()
            .map(|(i, k)| (k.clone(), i as u32))
            .collect();
        // The title listing: front articles by title (stable on ties).
        let mut fronts: Vec<(&[u8], u32)> = self
            .dirents
            .iter()
            .filter(|(_, d)| d.front)
            .map(|(k, d)| {
                (
                    if d.title.is_empty() {
                        &k.1[..]
                    } else {
                        &d.title[..]
                    },
                    index[k],
                )
            })
            .collect();
        fronts.sort_by(|a, b| a.0.cmp(b.0));
        let listing: Vec<u8> = fronts.iter().flat_map(|(_, i)| i.to_le_bytes()).collect();
        let (cluster, blob) = self.add_data(&listing, false)?;
        self.dirents.get_mut(&listing_key).unwrap().target =
            Target::Item(listing_mime, cluster, blob);

        if !self.comp.ends.is_empty() {
            self.close_cluster(true)?;
        }
        if !self.uncomp.ends.is_empty() {
            self.close_cluster(false)?;
        }

        // Mimetypes sorted, as libzim's `resolveMimeTypes`.
        let mut sorted = self.mimes.clone();
        sorted.sort();
        let remap: Vec<u16> = self
            .mimes
            .iter()
            .map(|m| sorted.iter().position(|s| s == m).unwrap() as u16)
            .collect();
        let mut mime_list = vec![];
        for m in &sorted {
            mime_list.extend(m.as_bytes());
            mime_list.push(0);
        }
        mime_list.push(0);
        if HEADER_SIZE + mime_list.len() as u64 >= CLUSTER_BASE_OFFSET {
            return Err(Error::Invalid(
                "the mimetype list doesn't fit before the clusters".into(),
            ));
        }

        let dirents = std::mem::take(&mut self.dirents);
        let uuid = match self.uuid {
            Some(u) => u,
            None => {
                let mut u = [0; 16];
                getrandom::fill(&mut u).map_err(|e| Error::Io(std::io::Error::other(e)))?;
                u
            }
        };
        let main_page = index
            .get(&(b'W', b"mainPage".to_vec()))
            .copied()
            .unwrap_or(u32::MAX);
        self.out()?;
        let mut f = self.out.take().unwrap();
        let mut write = || -> Result<()> {
            let mut dirent_offsets = Vec::with_capacity(dirents.len());
            let mut buf = vec![];
            for ((ns, path), d) in &dirents {
                let kind = match &d.target {
                    Target::Item(m, serial, blob) => Kind::Item {
                        mime: remap[*m as usize],
                        cluster: self.closed[serial].0,
                        blob: *blob,
                    },
                    Target::Redirect(tns, tp) => Kind::Redirect(index[&(*tns, tp.clone())]),
                    Target::Placeholder => unreachable!("placeholders are dropped"),
                };
                let dirent = Dirent {
                    kind,
                    ns: *ns,
                    path: path.clone(),
                    title: d.title.clone(),
                };
                buf.clear();
                dirent.encode(&mut buf);
                dirent_offsets.push(f.stream_position()?);
                f.write_all(&buf)?;
            }
            let path_ptr_pos = f.stream_position()?;
            for o in &dirent_offsets {
                f.write_all(&o.to_le_bytes())?;
            }
            let cluster_ptr_pos = f.stream_position()?;
            for o in &self.offsets {
                f.write_all(&o.to_le_bytes())?;
            }
            let header = Header {
                major: 6,
                minor: 3,
                uuid,
                entry_count: dirents.len() as u32,
                cluster_count: self.offsets.len() as u32,
                path_ptr_pos,
                title_idx_pos: u64::MAX,
                cluster_ptr_pos,
                mime_list_pos: HEADER_SIZE,
                main_page,
                layout_page: u32::MAX,
                checksum_pos: f.stream_position()?,
            };
            f.seek(SeekFrom::Start(0))?;
            f.write_all(&header.to_bytes())?;
            f.write_all(&mime_list)?;
            f.flush()?;
            // MD5 of everything so far, appended.
            let file = f.get_mut();
            file.seek(SeekFrom::Start(0))?;
            let mut md5 = Md5::new();
            let mut chunk = vec![0; 1 << 20];
            loop {
                let n = file.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                md5.update(&chunk[..n]);
            }
            file.write_all(&md5.finalize())?;
            file.sync_all()?;
            Ok(())
        };
        let r = write();
        drop(f);
        if r.is_err() {
            let _ = std::fs::remove_file(&self.tmp);
        }
        r?;
        std::fs::rename(&self.tmp, &self.path)?;
        Ok(())
    }
}

impl Drop for Creator {
    /// An unfinished archive leaves no `.tmp` behind.
    fn drop(&mut self) {
        if self.out.take().is_some() {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

/// libzim stores no title when it equals the path.
fn title_bytes(path: &str, title: &str) -> Vec<u8> {
    if title == path {
        vec![]
    } else {
        title.as_bytes().to_vec()
    }
}
