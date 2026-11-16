//! Header edge cases on a hand-made, empty archive (libzim test/archive.cpp).

use std::path::PathBuf;
use zimfile::Archive;

/// libzim test/archive.cpp:76 emptyZimArchiveContent.
fn empty_zim() -> Vec<u8> {
    let mut c = b"ZIM\x04".to_vec(); // magic
    c.extend([5, 0, 0, 0]); // version
    c.extend([0; 16]); // uuid
    c.extend([0; 4]); // article count
    c.extend([0; 4]); // cluster count
    for pos in [0x51u64, 0x51, 0x51, 0x50] {
        c.extend(pos.to_le_bytes()); // url, title, cluster, mimelist pointers
    }
    c.extend([0; 4]); // main page
    c.extend([0; 4]); // layout page
    c.extend(0x51u64.to_le_bytes()); // checksum pos
    c.push(0); // empty mimelist
    c.extend(b"\x9f\x3e\xcd\x95\x46\xf6\xc5\x3b\x35\xb4\xc6\xd4\xc0\x8e\xd0\x66"); // md5
    c
}

fn file(name: &str, content: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zimfile-header-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

// libzim test/archive.cpp:97 openingAnInvalidZimArchiveFails
#[test]
fn invalid_archives_fail() {
    for prefix in [&b"ZIM\x04"[..], b""] {
        for byte in [0x00u8, 0x01, 0x11, 0x30, 0xFF] {
            for count in (0..100).step_by(10) {
                let mut c = prefix.to_vec();
                c.extend(std::iter::repeat_n(byte, count));
                let p = file("invalid.zim", &c);
                assert!(Archive::open(&p).is_err(), "{prefix:?} {byte} {count}");
            }
        }
    }
}

// libzim test/archive.cpp:118 openingAnEmptyZimArchiveSucceeds
#[test]
fn empty_archive_opens() {
    let a = Archive::open(&file("empty.zim", &empty_zim())).unwrap();
    assert!(a.verify_checksum().unwrap());
    assert_eq!(a.article_count(), 0);
    assert_eq!(a.media_count(), 0);
}

// libzim test/archive.cpp:139 nastyEmptyZimArchive
#[test]
fn nasty_empty_archive() {
    let good = empty_zim();
    for offset in 0..80 {
        // Minor version, uuid, main and layout page: harmless.
        if (6..24).contains(&offset) || (64..72).contains(&offset) {
            continue;
        }
        let mut c = good.clone();
        c[offset] = 0xff;
        let p = file("nasty.zim", &c);
        assert!(Archive::open(&p).is_err(), "offset {offset}");
    }
}

// libzim test/archive.cpp:153 wrongChecksumInEmptyZimArchive
#[test]
fn wrong_checksum_in_empty_archive() {
    let mut c = empty_zim();
    c[85] = 0xff;
    let a = Archive::open(&file("bad_checksum.zim", &c)).unwrap();
    assert!(!a.verify_checksum().unwrap());
}
