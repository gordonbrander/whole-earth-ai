//! HTML → Markdown, shared by `read` and `export` so what the AI reads and
//! what you export are the same text. Pure functions.

use htmd::{Element, HtmlToMarkdown, Node, element_handler::Handlers};
use markup5ever_rcdom::NodeData;
use std::rc::Rc;

/// Decides what an `<a href>` becomes. `Some(url)` keeps a link to `url`;
/// `None` keeps only the link text.
pub type LinkFn = dyn Fn(&str) -> Option<String> + Send + Sync;

fn attr<'a>(el: &'a Element, name: &str) -> Option<&'a str> {
    el.attrs
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| &*a.value)
}

/// Wikipedia chrome that's noise to a reader or a model.
fn is_noise(el: &Element) -> bool {
    let class = attr(el, "class").unwrap_or("");
    let style = attr(el, "style").unwrap_or("").replace(' ', "");
    style.contains("display:none")
        || [
            "navbox",
            "mw-editsection",
            "noprint",
            "metadata",
            "mw-empty-elt",
        ]
        .iter()
        .any(|c| {
            class
                .split_whitespace()
                .any(|w| w == *c || w.starts_with(&format!("{c}-")))
        })
}

fn tag(node: &Node) -> Option<&str> {
    match &node.data {
        NodeData::Element { name, .. } => Some(&name.local),
        _ => None,
    }
}

/// An infobox as `- label: value` lines. As a Markdown table it would keep
/// only the first column, losing the labels.
fn infobox(h: &dyn Handlers, node: &Rc<Node>, out: &mut String) {
    for child in node.children.borrow().iter() {
        if tag(child) != Some("tr") {
            infobox(h, child, out); // thead/tbody
            continue;
        }
        let cells: Vec<String> = child
            .children
            .borrow()
            .iter()
            .filter(|c| matches!(tag(c), Some("th" | "td")))
            .map(|c| {
                h.walk_children(c)
                    .content
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|t| !t.is_empty())
            .collect();
        match cells.as_slice() {
            [] => {}
            [one] => *out += &format!("\n**{one}**\n"),
            [label, rest @ ..] => *out += &format!("- {label}: {}\n", rest.join(" ")),
        }
    }
}

/// Convert an article's HTML to Markdown, rewriting links with `link`.
pub fn to_markdown(html: &str, link: Box<LinkFn>) -> String {
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "head", "img", "noscript", "footer", "nav", "link", "meta",
        ])
        .add_handler(
            vec!["div", "span", "table", "ul", "ol", "p", "sup"],
            |h: &dyn Handlers, el: Element| {
                if is_noise(&el) {
                    Some("".into())
                } else if el.tag == "table"
                    && attr(&el, "class").is_some_and(|c| c.contains("infobox"))
                {
                    let mut out = String::from("\n\n");
                    infobox(h, el.node, &mut out);
                    Some((out + "\n").into())
                } else {
                    h.fallback(el)
                }
            },
        )
        .add_handler(vec!["a"], move |h: &dyn Handlers, el: Element| {
            let text = h.walk_children(el.node).content;
            match attr(&el, "href").and_then(&link) {
                Some(url) if !text.trim().is_empty() => {
                    Some(format!("[{}]({})", text.trim(), url.replace(' ', "%20")).into())
                }
                _ => Some(text.into()),
            }
        })
        .build();
    let body = content_html(html);
    let md = converter.convert(body).unwrap_or_default();
    // Drop whitespace-only lines and runs of blank lines left by removed chrome.
    let mut out = String::with_capacity(md.len());
    for line in md.lines().map(str::trim_end) {
        if !(line.is_empty() && (out.is_empty() || out.ends_with("\n\n"))) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// The article body: the `mw-content-text` element if present (MediaWiki
/// ZIMs), else the whole document. Avoids page chrome without a DOM query.
fn content_html(html: &str) -> &str {
    match html
        .find("id=\"mw-content-text\"")
        .and_then(|i| html[..i].rfind('<'))
    {
        Some(start) => &html[start..],
        None => html,
    }
}

/// Decode `%XX` escapes in a URL path.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolve an href found in the entry at `from` to an entry path in the same
/// archive. `None` for external links, fragments and anything else that
/// isn't another entry.
pub fn resolve_internal(from: &str, href: &str) -> Option<String> {
    if href.is_empty() || href.starts_with('#') || href.starts_with('/') || href.contains(':') {
        return None; // fragment, absolute, or has a scheme (http:, mailto:…)
    }
    let href = href.split(['#', '?']).next()?;
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop(); // the current entry's own name
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(percent_decode(&parts.join("/"))).filter(|p| !p.is_empty())
}

/// Folder a whole-archive export spreads files over: the first two
/// characters of the entry path, so no folder holds millions of files.
pub fn shard(path: &str) -> String {
    let s: String = path.chars().take(2).collect();
    s.replace(['/', '.'], "_")
}

/// Export file path for an entry, relative to the archive's export folder.
/// Entry paths may hold characters Windows and exFAT reject, and segments
/// longer than filesystems allow; those are replaced or shortened.
pub fn export_path(path: &str) -> String {
    let segments: Vec<String> = path
        .split('/')
        .map(|seg| {
            let mut s: String = seg
                .chars()
                .map(|c| {
                    if "<>:\"\\|?*".contains(c) || c.is_control() {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            if s.is_empty() || s == "." || s == ".." {
                s = format!("_{s}");
            }
            if s.len() > 200 {
                // Keep it unique with a hash of the full segment (FNV-1a).
                let h = seg.bytes().fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ b as u64).wrapping_mul(0x100000001b3)
                });
                let mut cut = 180;
                while !s.is_char_boundary(cut) {
                    cut -= 1;
                }
                s = format!("{}~{h:016x}", &s[..cut]);
            }
            s
        })
        .collect();
    format!("{}/{}.md", shard(path), segments.join("/"))
}

/// Relative link from one exported file to another.
pub fn relative_md_link(from: &str, to: &str) -> String {
    let from = export_path(from);
    let depth = from.matches('/').count();
    format!("{}{}", "../".repeat(depth), export_path(to))
}

/// Split Markdown into pages of roughly `size` bytes at paragraph breaks.
/// A paragraph longer than `2 * size` (a big table, say) is cut into chunks.
pub fn paginate(md: &str, size: usize) -> Vec<String> {
    let mut pages = vec![];
    let mut cur = String::new();
    for para in md.split("\n\n") {
        let mut rest = para;
        loop {
            let mut end = rest.len();
            if end > size * 2 {
                end = size;
                while !rest.is_char_boundary(end) {
                    end -= 1;
                }
            }
            // Break only once the page holds something worth reading, so a short
            // hatnote isn't left alone on page 1 before a big infobox.
            if cur.len() >= size / 2 && cur.len() + end > size {
                pages.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push_str("\n\n");
            }
            cur.push_str(&rest[..end]);
            rest = &rest[end..];
            if rest.is_empty() {
                break;
            }
        }
    }
    pages.push(cur);
    pages
}

/// Section headings (`##`…) in order: the article's outline.
pub fn outline(md: &str) -> Vec<String> {
    md.lines()
        .filter(|l| l.starts_with("## ") || l.starts_with("### "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .collect()
}

/// The Markdown of one section (heading through to the next heading of the
/// same or higher level), matched case-insensitively.
pub fn section(md: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = md.lines().collect();
    let level = |l: &str| l.chars().take_while(|c| *c == '#').count();
    let start = lines.iter().position(|l| {
        level(l) >= 2
            && l.trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case(name.trim())
    })?;
    let lvl = level(lines[start]);
    let end = lines[start + 1..]
        .iter()
        .position(|l| (1..=lvl).contains(&level(l)) && l.chars().nth(level(l)) == Some(' '))
        .map_or(lines.len(), |i| start + 1 + i);
    Some(lines[start..end].join("\n"))
}

/// Remove HTML tags (libzim snippets carry `<b>` highlights).
pub fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Truncate to at most `max` chars, adding an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_resolve_and_rewrite() {
        assert_eq!(resolve_internal("A/Foo", "Bar"), Some("A/Bar".into()));
        assert_eq!(
            resolve_internal("Foo", "./Sapa%28IOL%29#x"),
            Some("Sapa(IOL)".into())
        );
        assert_eq!(resolve_internal("A/Foo", "../B/Bar"), Some("B/Bar".into()));
        assert_eq!(resolve_internal("Foo", "https://x.org/a"), None);
        assert_eq!(resolve_internal("Foo", "#cite"), None);
        assert_eq!(relative_md_link("Foo", "Bar"), "../Ba/Bar.md");
        assert_eq!(relative_md_link("A/Foo", "A/Bar"), "../../A_/A/Bar.md");
        assert_eq!(export_path("What?_\"Yes\""), "Wh/What___Yes_.md");
        assert!(export_path(&"x".repeat(300)).len() < 210);
        assert_ne!(
            export_path(&format!("{}a", "x".repeat(300))),
            export_path(&format!("{}b", "x".repeat(300)))
        );
    }

    #[test]
    fn converts_and_strips_noise() {
        let html = r#"<html><head><title>x</title></head><body><nav>menu</nav>
            <div id="mw-content-text"><h2>History</h2><p>See <a href="./Rome">Rome</a> and
            <a href="https://e.org">e</a>.<span style="display: none">hidden</span></p>
            <table class="navbox"><tr><td>nav</td></tr></table><ul><li>one</li></ul>
            <table class="infobox vcard"><tr><th colspan="2">Mount Thor</th></tr>
            <tr><th class="infobox-label">Elevation</th><td>1,675 m</td></tr></table></div></body></html>"#;
        let md = to_markdown(
            html,
            Box::new(|h| resolve_internal("Here", h).map(|p| format!("{p}.md"))),
        );
        assert!(md.contains("## History"), "{md}");
        assert!(md.contains("[Rome](Rome.md)"), "{md}");
        assert!(md.contains("e."), "{md}");
        assert!(!md.contains("https://e.org") || md.contains("[e]("), "{md}");
        assert!(
            !md.contains("hidden") && !md.contains("nav") && !md.contains("menu"),
            "{md}"
        );
        assert!(md.contains("one"), "{md}");
        assert!(
            md.contains("**Mount Thor**") && md.contains("- Elevation: 1,675 m"),
            "{md}"
        );
    }

    #[test]
    fn pages_sections_snippets() {
        let md = "# T\n\nintro\n\n## A\n\na text\n\n### A1\n\nsub\n\n## B\n\nb text";
        assert_eq!(outline(md), vec!["A", "A1", "B"]);
        assert_eq!(
            section(md, "a").unwrap(),
            "## A\n\na text\n\n### A1\n\nsub\n"
        );
        assert_eq!(section(md, "B").unwrap(), "## B\n\nb text");
        let pages = paginate(md, 20);
        assert!(pages.len() > 1);
        assert_eq!(pages.join("\n\n"), md);
        let big = "x".repeat(100);
        assert_eq!(paginate(&big, 10).concat(), big);
        assert_eq!(strip_tags("a <b>b</b>"), "a b");
        assert_eq!(truncate("héllo world", 5), "héllo…");
        assert_eq!(percent_decode("a%20b%zz"), "a b%zz");
    }
}
