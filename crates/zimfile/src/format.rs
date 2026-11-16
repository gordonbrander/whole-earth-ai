//! The on-disk structures: header, dirent, cluster. Little-endian throughout.
//! Ported from libzim's `fileheader.cpp`, `dirent.cpp` and `cluster.cpp`.

use crate::{Error, Result};
use std::io::Read;

pub const MAGIC: u32 = 0x044D_495A;
pub const HEADER_SIZE: u64 = 80;
pub const REDIRECT: u16 = 0xFFFF;
pub const LINKTARGET: u16 = 0xFFFE;
pub const DELETED: u16 = 0xFFFD;

fn bad(msg: impl Into<String>) -> Error {
    Error::Format(msg.into())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub major: u16,
    pub minor: u16,
    pub uuid: [u8; 16],
    pub entry_count: u32,
    pub cluster_count: u32,
    pub path_ptr_pos: u64,
    pub title_idx_pos: u64,
    pub cluster_ptr_pos: u64,
    pub mime_list_pos: u64,
    pub main_page: u32,
    pub layout_page: u32,
    pub checksum_pos: u64,
}

impl Header {
    pub fn parse(b: &[u8]) -> Result<Header> {
        let u16_ = |o: usize| u16::from_le_bytes(b[o..o + 2].try_into().unwrap());
        let u32_ = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let u64_ = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        if b.len() < HEADER_SIZE as usize {
            return Err(bad("zim-file is too small to contain a header"));
        }
        if u32_(0) != MAGIC {
            return Err(bad("Invalid magic number"));
        }
        let h = Header {
            major: u16_(4),
            minor: u16_(6),
            uuid: b[8..24].try_into().unwrap(),
            entry_count: u32_(24),
            cluster_count: u32_(28),
            path_ptr_pos: u64_(32),
            title_idx_pos: u64_(40),
            cluster_ptr_pos: u64_(48),
            mime_list_pos: u64_(56),
            main_page: u32_(64),
            layout_page: u32_(68),
            checksum_pos: u64_(72),
        };
        if h.major != 5 && h.major != 6 {
            return Err(bad("Invalid version"));
        }
        h.sanity_check()?;
        Ok(h)
    }

    /// `Fileheader::sanity_check`.
    fn sanity_check(&self) -> Result<()> {
        let m = self.mime_list_pos;
        if (self.entry_count == 0) != (self.cluster_count == 0) {
            return Err(bad("No article <=> No cluster"));
        }
        if m != HEADER_SIZE && m != 72 {
            return Err(bad("mimelistPos must be 80."));
        }
        if self.path_ptr_pos < m {
            return Err(bad("pathPtrPos must be > mimelistPos."));
        }
        if self.has_title_v0() && self.title_idx_pos < m {
            return Err(bad("titleIdxPos must be > mimelistPos."));
        }
        if self.cluster_ptr_pos < m {
            return Err(bad("clusterPtrPos must be > mimelistPos."));
        }
        if self.cluster_count > self.entry_count {
            return Err(bad("Cluster count cannot be higher than article count."));
        }
        if self.checksum_pos != 0 && self.checksum_pos < m {
            return Err(bad("checksumPos must be > mimeListPos."));
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> [u8; 80] {
        let mut b = Vec::with_capacity(80);
        b.extend(MAGIC.to_le_bytes());
        b.extend(self.major.to_le_bytes());
        b.extend(self.minor.to_le_bytes());
        b.extend(self.uuid);
        b.extend(self.entry_count.to_le_bytes());
        b.extend(self.cluster_count.to_le_bytes());
        for v in [
            self.path_ptr_pos,
            self.title_idx_pos,
            self.cluster_ptr_pos,
            self.mime_list_pos,
        ] {
            b.extend(v.to_le_bytes());
        }
        b.extend(self.main_page.to_le_bytes());
        b.extend(self.layout_page.to_le_bytes());
        b.extend(self.checksum_pos.to_le_bytes());
        b.try_into().unwrap()
    }

    pub fn has_title_v0(&self) -> bool {
        self.title_idx_pos != u64::MAX
    }
    pub fn has_checksum(&self) -> bool {
        self.mime_list_pos >= HEADER_SIZE
    }
    pub fn new_namespaces(&self) -> bool {
        self.minor >= 1
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Item {
        mime: u16,
        cluster: u32,
        blob: u32,
    },
    Redirect(u32),
    /// Linktarget or deleted: no content.
    Other(u16),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dirent {
    pub kind: Kind,
    pub ns: u8,
    pub path: Vec<u8>,
    /// As stored: empty means "same as path".
    pub title: Vec<u8>,
}

impl Dirent {
    /// `DirentReader::initDirent`. `Ok(None)` means `b` is too short: read more.
    pub fn parse(b: &[u8]) -> Result<Option<Dirent>> {
        if b.len() < 8 {
            return Ok(None);
        }
        let mime = u16::from_le_bytes([b[0], b[1]]);
        let extra = b[2] as usize;
        let ns = b[3];
        let u32_ = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let (kind, mut p) = match mime {
            REDIRECT if b.len() >= 12 => (Kind::Redirect(u32_(8)), 12),
            LINKTARGET | DELETED => (Kind::Other(mime), 8),
            REDIRECT => return Ok(None),
            _ if b.len() >= 16 => (
                Kind::Item {
                    mime,
                    cluster: u32_(8),
                    blob: u32_(12),
                },
                16,
            ),
            _ => return Ok(None),
        };
        // strnlen bounds: a string can't run into the extra bytes.
        let mut cstr = || {
            let left = &b[p..];
            let max = left.len().saturating_sub(extra);
            let n = left[..max].iter().position(|&c| c == 0).unwrap_or(max);
            if n >= left.len() {
                return None;
            }
            p += n + 1;
            Some(left[..n].to_vec())
        };
        let Some(path) = cstr() else { return Ok(None) };
        let Some(title) = cstr() else { return Ok(None) };
        if extra > b.len() - p {
            return Ok(None);
        }
        Ok(Some(Dirent {
            kind,
            ns,
            path,
            title,
        }))
    }

    pub fn title(&self) -> &[u8] {
        if self.title.is_empty() {
            &self.path
        } else {
            &self.title
        }
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        let mime = match self.kind {
            Kind::Item { mime, .. } => mime,
            Kind::Redirect(_) => REDIRECT,
            Kind::Other(m) => m,
        };
        out.extend(mime.to_le_bytes());
        out.push(0); // no extra parameters
        out.push(self.ns);
        out.extend(0u32.to_le_bytes()); // revision
        match self.kind {
            Kind::Item { cluster, blob, .. } => {
                out.extend(cluster.to_le_bytes());
                out.extend(blob.to_le_bytes());
            }
            Kind::Redirect(t) => out.extend(t.to_le_bytes()),
            Kind::Other(_) => {}
        }
        out.extend(&self.path);
        out.push(0);
        out.extend(&self.title);
        out.push(0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Compression {
    None,
    Lzma,
    Zstd,
}

impl Compression {
    pub fn from_info(info: u8) -> Result<Compression> {
        match info & 0x0F {
            0 | 1 => Ok(Compression::None), // very old files used 0 for none
            2 => Err(bad("zlib not enabled in this library")),
            3 => Err(bad("bzip2 not enabled in this library")),
            4 => Ok(Compression::Lzma),
            5 => Ok(Compression::Zstd),
            _ => Err(bad("Invalid compression flag")),
        }
    }
}

/// A cluster's decompressed bytes. `offsets` are relative to the start of
/// `data` (which begins with the offset table): blob i is `off[i]..off[i+1]`.
#[derive(Debug)]
pub struct Cluster {
    pub compression: Compression,
    pub offsets: Vec<u64>,
    pub data: Vec<u8>,
}

impl Cluster {
    /// Read a cluster from `r` (positioned at its info byte). `max_blobs` is
    /// the archive's entry count, as in libzim. Reads only as far as the last blob.
    pub fn read(mut r: impl Read, max_blobs: u64) -> Result<Cluster> {
        let mut info = [0];
        r.read_exact(&mut info)?;
        let compression = Compression::from_info(info[0])?;
        let wide = info[0] & 0x10 != 0;
        match compression {
            Compression::None => Cluster::read_body(r, compression, wide, max_blobs),
            Compression::Zstd => {
                let d = zstd::stream::read::Decoder::with_buffer(std::io::BufReader::new(r))?
                    .single_frame();
                Cluster::read_body(d, compression, wide, max_blobs)
            }
            Compression::Lzma => {
                let d = liblzma::read::XzDecoder::new(std::io::BufReader::new(r));
                Cluster::read_body(d, compression, wide, max_blobs)
            }
        }
    }

    /// `Cluster::read_header`, then the blob bytes.
    fn read_body(
        mut r: impl Read,
        compression: Compression,
        wide: bool,
        max_blobs: u64,
    ) -> Result<Cluster> {
        let w: u64 = if wide { 8 } else { 4 };
        let mut data = vec![0; w as usize];
        r.read_exact(&mut data)?;
        let word = |b: &[u8]| {
            if wide {
                u64::from_le_bytes(b.try_into().unwrap())
            } else {
                u32::from_le_bytes(b.try_into().unwrap()) as u64
            }
        };
        let first = word(&data);
        if first != w && first < 2 * w {
            return Err(bad(
                "Error parsing cluster. Offset of the first blob is too small.",
            ));
        }
        let n = first / w;
        if n * w != first {
            return Err(bad(
                "Error parsing cluster. Offset of the first blob is not properly aligned.",
            ));
        }
        if n > max_blobs + 1 {
            return Err(bad(
                "Error parsing cluster. Offset of the first blob is too large.",
            ));
        }
        data.resize(first as usize, 0);
        r.read_exact(&mut data[w as usize..])?;
        let offsets: Vec<u64> = data.chunks(w as usize).map(word).collect();
        if offsets.windows(2).any(|p| p[1] < p[0]) {
            return Err(bad("Error parsing cluster. Offsets are not ordered."));
        }
        let end = *offsets.last().unwrap();
        // Grows as it reads: a corrupt huge offset fails on EOF, not on allocation.
        (&mut r).take(end - first).read_to_end(&mut data)?;
        if (data.len() as u64) < end {
            return Err(bad("cluster is truncated"));
        }
        Ok(Cluster {
            compression,
            offsets,
            data,
        })
    }

    pub fn count(&self) -> u32 {
        (self.offsets.len() - 1) as u32
    }

    /// Blob `i`, or empty when out of range (as libzim's `Cluster::getBlob`).
    pub fn blob(&self, i: u32) -> &[u8] {
        if i >= self.count() {
            return &[];
        }
        let i = i as usize;
        &self.data[self.offsets[i] as usize..self.offsets[i + 1] as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            major: 6,
            minor: 3,
            uuid: [7; 16],
            entry_count: 3,
            cluster_count: 1,
            path_ptr_pos: 1000,
            title_idx_pos: u64::MAX,
            cluster_ptr_pos: 2000,
            mime_list_pos: 80,
            main_page: u32::MAX,
            layout_page: u32::MAX,
            checksum_pos: 3000,
        }
    }

    #[test]
    fn header_round_trip_and_sanity() {
        let h = header();
        assert_eq!(Header::parse(&h.to_bytes()).unwrap(), h);
        let broken = [
            Header {
                mime_list_pos: 81,
                ..h.clone()
            },
            Header {
                path_ptr_pos: 10,
                ..h.clone()
            },
            Header {
                cluster_ptr_pos: 10,
                ..h.clone()
            },
            Header {
                title_idx_pos: 10,
                ..h.clone()
            },
            Header {
                cluster_count: 4,
                ..h.clone()
            },
            Header {
                cluster_count: 0,
                ..h.clone()
            },
            Header {
                checksum_pos: 10,
                ..h.clone()
            },
            Header {
                major: 7,
                ..h.clone()
            },
        ];
        for b in broken {
            assert!(Header::parse(&b.to_bytes()).is_err(), "{b:?}");
        }
        let mut bytes = h.to_bytes();
        bytes[0] = 0;
        assert!(Header::parse(&bytes).is_err());
        assert!(Header::parse(&bytes[..79]).is_err());
    }

    #[test]
    fn dirents() {
        let d = Dirent {
            kind: Kind::Item {
                mime: 1,
                cluster: 2,
                blob: 3,
            },
            ns: b'C',
            path: b"foo".to_vec(),
            title: b"Foo".to_vec(),
        };
        let mut b = vec![];
        d.encode(&mut b);
        assert_eq!(Dirent::parse(&b).unwrap(), Some(d.clone()));
        // Too short: ask for more bytes.
        for n in 0..b.len() {
            assert_eq!(Dirent::parse(&b[..n]).unwrap(), None, "{n}");
        }
        let r = Dirent {
            kind: Kind::Redirect(9),
            title: vec![],
            ..d
        };
        let mut b = vec![];
        r.encode(&mut b);
        b.extend([0xAA; 10]); // trailing bytes are ignored
        let p = Dirent::parse(&b).unwrap().unwrap();
        assert_eq!(p, r);
        assert_eq!(p.title(), b"foo");
        // Extra parameter bytes come after the title.
        let mut b = vec![1, 0, 2, b'C', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        b.extend(b"p\0t\0xy");
        let p = Dirent::parse(&b).unwrap().unwrap();
        assert_eq!(
            (p.path.as_slice(), p.title.as_slice()),
            (&b"p"[..], &b"t"[..])
        );
        assert_eq!(Dirent::parse(&b[..b.len() - 1]).unwrap(), None);
    }

    fn raw(offsets: &[u32], body: &[u8]) -> Vec<u8> {
        let mut b = vec![1];
        for o in offsets {
            b.extend(o.to_le_bytes());
        }
        b.extend(body);
        b
    }

    #[test]
    fn clusters() {
        let c = Cluster::read(&raw(&[12, 15, 17], b"abcde")[..], 10).unwrap();
        assert_eq!(
            (c.count(), c.blob(0), c.blob(1), c.blob(2)),
            (2, &b"abc"[..], &b"de"[..], &b""[..])
        );
        // An empty cluster: one offset.
        assert_eq!(Cluster::read(&raw(&[4], b"")[..], 10).unwrap().count(), 0);
        for (offs, what) in [
            (&[5u32, 6][..], "too small"),
            (&[2][..], "too small"),
            (&[10, 12, 13][..], "misaligned"),
            (&[16, 16, 17, 17][..], "too large"),
            (&[12, 16, 15][..], "not ordered"),
            (&[12, 15, 99][..], "truncated"),
        ] {
            assert!(
                Cluster::read(&raw(offs, b"abcde")[..], 2).is_err(),
                "{what}"
            );
        }
        let mut z = vec![5];
        z.extend(zstd::encode_all(&raw(&[8, 11], b"xyz")[1..], 3).unwrap());
        z.extend(b"trailing bytes are not read");
        assert_eq!(Cluster::read(&z[..], 1).unwrap().blob(0), b"xyz");
        let mut x = vec![4];
        let body = raw(&[8, 11], b"xyz");
        let mut e = liblzma::read::XzEncoder::new(&body[1..], 6);
        e.read_to_end(&mut x).unwrap();
        x.extend(b"trailing");
        assert_eq!(Cluster::read(&x[..], 1).unwrap().blob(0), b"xyz");
        assert!(Cluster::read(&[2u8, 0, 0][..], 1).is_err()); // zlib
        let mut wide = vec![0x11];
        for o in [16u64, 18] {
            wide.extend(o.to_le_bytes());
        }
        wide.extend(b"hi");
        assert_eq!(Cluster::read(&wide[..], 1).unwrap().blob(0), b"hi");
    }
}
