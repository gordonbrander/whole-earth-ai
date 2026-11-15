//! Kiwix's online catalog (OPDS feed) and the `.meta4` metalink files that
//! give each archive's exact size, sha256 and mirrors.

use super::parse_filename;
use quick_xml::{Reader, XmlVersion, events::Event};
use serde::Serialize;

const CATALOG: &str = "https://opds.library.kiwix.org/catalog/v2/entries";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CatalogEntry {
    /// Archive name as it appears in filenames, e.g. `wikipedia_en_all_maxi`.
    pub name: String,
    pub date: String,
    pub title: String,
    pub summary: String,
    pub language: String,
    pub category: String,
    pub size: u64,
    pub meta4_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Meta4 {
    pub file: String,
    pub size: u64,
    pub sha256: String,
    /// Mirrors in priority order.
    pub urls: Vec<String>,
}

pub fn get(url: &str) -> Result<String, String> {
    ureq::get(url)
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("{url}: {e}"))
}

fn query_string(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| {
            let v: String = v
                .bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect();
            format!("{k}={v}")
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Search the catalog by free text, language (ISO 639-3, e.g. `eng`) and category.
pub fn search(
    query: &str,
    lang: &str,
    category: &str,
    count: u32,
) -> Result<Vec<CatalogEntry>, String> {
    let count = count.to_string();
    let qs = query_string(&[
        ("q", query),
        ("lang", lang),
        ("category", category),
        ("count", &count),
    ]);
    Ok(parse_feed(&get(&format!("{CATALOG}?{qs}"))?))
}

/// The newest catalog entry for an archive name (`wikipedia_en_all_maxi`).
/// The catalog's own `name` leaves out the flavour (`_maxi`), so if the full
/// name finds nothing, try again without its last part.
pub fn find(name: &str) -> Result<Option<CatalogEntry>, String> {
    let mut candidates = vec![name];
    if let Some((base, _flavour)) = name.rsplit_once('_') {
        candidates.push(base);
    }
    for n in candidates {
        let qs = query_string(&[("name", n), ("count", "-1")]);
        let entries = parse_feed(&get(&format!("{CATALOG}?{qs}"))?);
        if let Some(e) = entries
            .into_iter()
            .filter(|e| e.name == name)
            .max_by(|a, b| a.date.cmp(&b.date))
        {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

/// Append text to the entry field being read.
fn push(c: &mut CatalogEntry, field: &str, v: &str) {
    match field {
        "title" => c.title += v,
        "summary" => c.summary += v,
        "language" => c.language += v,
        "category" => c.category += v,
        _ => {}
    }
}

/// Parse an OPDS acquisition feed. Entries without a ZIM download are skipped.
pub fn parse_feed(xml: &str) -> Vec<CatalogEntry> {
    let mut reader = Reader::from_str(xml);
    let mut out = vec![];
    let mut cur: Option<CatalogEntry> = None;
    let mut field = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let tag = e.local_name().as_ref().to_string();
                if tag == "entry" {
                    cur = Some(CatalogEntry {
                        name: String::new(),
                        date: String::new(),
                        title: String::new(),
                        summary: String::new(),
                        language: String::new(),
                        category: String::new(),
                        size: 0,
                        meta4_url: String::new(),
                    });
                } else if tag == "link" {
                    let attr = |k: &str| {
                        e.try_get_attribute(k)
                            .ok()
                            .flatten()
                            .and_then(|a| a.normalized_value(XmlVersion::default()).ok())
                            .map(|v| v.into_owned())
                    };
                    if let (Some(c), Some(href)) = (cur.as_mut(), attr("href"))
                        && attr("type").as_deref() == Some("application/x-zim")
                    {
                        c.size = attr("length").and_then(|l| l.parse().ok()).unwrap_or(0);
                        let file = href
                            .rsplit('/')
                            .next()
                            .unwrap_or("")
                            .trim_end_matches(".meta4");
                        if let Some((name, date)) = parse_filename(file) {
                            c.name = name;
                            c.date = date;
                        }
                        c.meta4_url = href;
                    }
                }
                field = tag;
            }
            Ok(Event::Text(t)) => {
                if let Some(c) = cur.as_mut() {
                    push(c, &field, &t.xml_content(XmlVersion::default()));
                }
            }
            Ok(Event::GeneralRef(r)) => {
                // Entities like &amp; arrive separately from the text around them.
                if let Some(c) = cur.as_mut() {
                    let named = match r.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    };
                    let v = r
                        .resolve_char_ref()
                        .ok()
                        .flatten()
                        .map_or(named.to_string(), String::from);
                    push(c, &field, &v);
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == "entry"
                    && let Some(c) = cur.take().filter(|c| !c.meta4_url.is_empty())
                {
                    out.push(c);
                }
                field.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Parse a metalink v4 file (first `<file>` only).
pub fn parse_meta4(xml: &str) -> Result<Meta4, String> {
    let mut reader = Reader::from_str(xml);
    let mut m = Meta4 {
        file: String::new(),
        size: 0,
        sha256: String::new(),
        urls: vec![],
    };
    let mut field = String::new();
    let mut hash_type = String::new();
    let mut urls: Vec<(u32, String)> = vec![];
    let mut priority = 0;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = e.local_name().as_ref().to_string();
                let attr = |k: &str| {
                    e.try_get_attribute(k)
                        .ok()
                        .flatten()
                        .and_then(|a| a.normalized_value(XmlVersion::default()).ok())
                        .map(|v| v.into_owned())
                };
                match tag.as_str() {
                    "file" if m.file.is_empty() => m.file = attr("name").unwrap_or_default(),
                    "hash" => hash_type = attr("type").unwrap_or_default(),
                    "url" => {
                        priority = attr("priority").and_then(|p| p.parse().ok()).unwrap_or(999)
                    }
                    _ => {}
                }
                field = tag;
            }
            Ok(Event::Text(t)) => {
                let v = t.xml_content(XmlVersion::default()).trim().to_string();
                match field.as_str() {
                    "size" if m.size == 0 => m.size = v.parse().unwrap_or(0),
                    "hash" if hash_type == "sha-256" && m.sha256.is_empty() => {
                        m.sha256 = v.to_lowercase()
                    }
                    "url" if !v.is_empty() => urls.push((priority, v)),
                    _ => {}
                }
            }
            Ok(Event::End(_)) => {
                field.clear();
                hash_type.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("bad metalink: {e}")),
            _ => {}
        }
    }
    urls.sort_by_key(|(p, _)| *p);
    m.urls = urls.into_iter().map(|(_, u)| u).collect();
    if m.file.is_empty() || m.size == 0 || m.sha256.len() != 64 || m.urls.is_empty() {
        return Err("metalink is missing the file name, size, sha256 or mirrors".into());
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/terms/">
  <totalResults>2</totalResults>
  <entry>
    <id>urn:uuid:1</id>
    <title>Wikipedia &amp; friends</title>
    <summary>Top articles</summary>
    <language>eng</language>
    <name>wikipedia_en_all</name>
    <flavour>maxi</flavour>
    <category>wikipedia</category>
    <link type="text/html" href="https://browse.library.kiwix.org/content/x" />
    <link rel="http://opds-spec.org/acquisition/open-access" type="application/x-zim" href="https://lb.download.kiwix.org/zim/wikipedia/wikipedia_en_all_maxi_2026-08.zim.meta4" length="127418088448" />
  </entry>
  <entry><title>No download</title></entry>
</feed>"#;

    const META4: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="wikipedia_en_100_2026-08.zim">
    <size>333429922</size>
    <hash type="md5">e00f3861b76dd768ab73db8fcb757bc2</hash>
    <hash type="sha-256">2EB7FD115BFB8644A08A9482F01F52B507428B1B8A30129FEAD92F7B6715440B</hash>
    <pieces length="4194304" type="sha-1"><hash>dc196e847b8a24ac181787bbfe139ef0d1833041</hash></pieces>
    <url location="fr" priority="2">https://b.example/x.zim</url>
    <url location="nl" priority="1">https://a.example/x.zim</url>
  </file>
</metalink>"#;

    #[test]
    fn parses_feed() {
        let e = parse_feed(FEED);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].name, "wikipedia_en_all_maxi");
        assert_eq!(e[0].date, "2026-08");
        assert_eq!(e[0].title, "Wikipedia & friends");
        assert_eq!(e[0].language, "eng");
        assert_eq!(e[0].size, 127418088448);
    }

    #[test]
    fn parses_meta4() {
        let m = parse_meta4(META4).unwrap();
        assert_eq!(m.file, "wikipedia_en_100_2026-08.zim");
        assert_eq!(m.size, 333429922);
        assert_eq!(
            m.sha256,
            "2eb7fd115bfb8644a08a9482f01f52b507428b1b8a30129fead92f7b6715440b"
        );
        assert_eq!(
            m.urls,
            vec!["https://a.example/x.zim", "https://b.example/x.zim"]
        );
        assert!(parse_meta4("<metalink/>").is_err());
        assert_eq!(
            query_string(&[("q", "ray charles"), ("lang", "")]),
            "q=ray%20charles"
        );
    }
}
