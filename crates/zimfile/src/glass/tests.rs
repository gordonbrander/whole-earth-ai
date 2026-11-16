//! The reader against the real indexes of the climate archive (`just testdata`),
//! values decoded independently with `xapian-delve`.

use super::*;
use crate::Archive;
use std::path::PathBuf;

fn climate() -> Archive {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/nons/wikipedia_en_climate_change_mini_2024-06.zim");
    Archive::open(&p).expect("test data missing: run `just testdata`")
}

#[test]
fn version_block() {
    let a = climate();
    let ft = a.xapian(true).unwrap();
    assert_eq!(&ft.uuid[..4], &[0x62, 0x95, 0xd1, 0x47]);
    assert_eq!(ft.revision, 1);
    assert_eq!(ft.blocksize(), 8192);
    let pl = &ft.roots[db::POSTLIST];
    assert_eq!((pl.root, pl.level, pl.entries), (3, 1, 27034));
    assert!(ft.roots[db::POSITION].fake && ft.roots[2].fake);
    assert!(!ft.has_positions());
    let s = &ft.stats;
    assert_eq!(
        (s.doccount, s.last_docid, s.total_doclen),
        (3821, 3821, 797963)
    );
    assert_eq!(
        (s.doclen_lbound, s.doclen_ubound, s.wdf_ubound),
        (6, 1617, 105)
    );
    let t = a.xapian(false).unwrap();
    assert_eq!(t.stats.doccount, 3569);
    assert!(t.has_positions());
}

#[test]
fn docdata_metadata_terms() {
    let a = climate();
    let ft = a.xapian(true).unwrap();
    assert_eq!(
        ft.data(5).unwrap().as_deref(),
        Some(&b"C/Albert_Sustainable_Production_Certification"[..])
    );
    assert_eq!(ft.data(99999).unwrap(), None);
    assert_eq!(
        ft.metadata("language").unwrap().as_deref(),
        Some(&b"eng"[..])
    );
    assert_eq!(
        ft.metadata("kind").unwrap().as_deref(),
        Some(&b"fulltext"[..])
    );
    assert_eq!(ft.metadata("stopwords").unwrap(), None);
    assert_eq!(ft.freqs(b"climat").unwrap(), (2414, 10917));
    assert_eq!(ft.freqs(b"nosuchterm").unwrap(), (0, 0));
    let terms = ft.terms(b"climat").unwrap();
    assert_eq!(terms[0], (b"climat".to_vec(), 2414));
    assert!(terms.iter().all(|(t, _)| t.starts_with(b"climat")));
    assert!(terms.windows(2).all(|w| w[0].0 < w[1].0));
}

#[test]
fn postlists_and_doclens() {
    let a = climate();
    let ft = a.xapian(true).unwrap();
    // Every posting of "climat", walked and then skipped to.
    let mut pl = ft.postlist(b"climat").unwrap().unwrap();
    let mut all = vec![];
    while pl.next().unwrap() {
        all.push((pl.did(), pl.wdf()));
    }
    assert_eq!(all.len(), 2414);
    assert_eq!(all[0].0, 1);
    assert_eq!(all.iter().map(|x| x.1 as u64).sum::<u64>(), 10917);
    assert!(all.windows(2).all(|w| w[0].0 < w[1].0));
    for step in [1, 7, 100, 1000] {
        let mut pl = ft.postlist(b"climat").unwrap().unwrap();
        for &(did, wdf) in all.iter().step_by(step) {
            assert!(pl.skip_to(did).unwrap());
            assert_eq!((pl.did(), pl.wdf()), (did, wdf));
        }
        assert!(!pl.skip_to(all.last().unwrap().0 + 1).unwrap());
    }
    // Skipping to a docid between postings lands on the next one.
    let mut pl = ft.postlist(b"climat").unwrap().unwrap();
    let gap = all.windows(2).find(|w| w[1].0 > w[0].0 + 1).unwrap();
    assert!(pl.skip_to(gap[0].0 + 1).unwrap());
    assert_eq!(pl.did(), gap[1].0);
    // Document lengths: one per document, summing to total_doclen.
    let mut dl = ft.doclens().unwrap().unwrap();
    let (mut n, mut sum) = (0, 0u64);
    while dl.next().unwrap() {
        n += 1;
        sum += dl.wdf() as u64;
    }
    assert_eq!((n, sum), (3821, 797963));
    let mut dl = ft.doclens().unwrap().unwrap();
    assert!(dl.skip_to(3000).unwrap());
    assert_eq!(dl.did(), 3000);
}

#[test]
fn values_and_positions() {
    let a = climate();
    let ft = a.xapian(true).unwrap();
    assert_eq!(
        ft.value(5, 0).unwrap().as_deref(),
        Some(&b"albert sustainable production certification"[..])
    );
    let wc = ft.value(5, 1).unwrap().unwrap();
    assert!(String::from_utf8(wc).unwrap().parse::<u32>().unwrap() > 0);
    assert_eq!(ft.value(5, 9).unwrap(), None);
    assert_eq!(ft.value_stats(0).unwrap().unwrap().0, 3821);
    let t = a.xapian(false).unwrap();
    // "0posanchor" is at position 1 of every title document.
    for did in [1, 2, 100, 3569] {
        assert_eq!(t.positions(did, b"0posanchor").unwrap(), vec![1]);
    }
}
