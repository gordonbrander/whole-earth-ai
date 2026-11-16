//! Write a tiny ZIM: `cargo run -p zimfile --example write -- hello.zim`.
//! Open it with `zim-mcp read hello/hello --dir .`, kiwix-serve or zimcheck.

use zimfile::{Creator, Illustration, Item};

fn main() -> zimfile::Result<()> {
    let out = std::env::args().nth(1).unwrap_or("hello.zim".into());
    let mut c = Creator::new(out.as_ref()).main_path("hello");
    for (name, value) in [
        ("Title", "Hello"),
        ("Language", "eng"),
        ("Creator", "zimfile"),
        ("Publisher", "zimfile"),
        ("Date", "2026-11-16"),
        ("Description", "A tiny example archive"),
        ("Name", "hello"),
    ] {
        c.add_metadata(name, value.as_bytes(), "text/plain")?;
    }
    c.add_illustration(&Illustration::square(48), include_bytes!("dot.png"))?;
    let page = "<html><head><title>Hello</title></head><body><h1>Hello</h1>\
                <p>Written by <a href=\"world\">zimfile</a>.</p></body></html>";
    c.add_item(Item::new("hello", "Hello", "text/html", page))?;
    let world = "<html><head><title>World</title></head><body><p>World.</p></body></html>";
    c.add_item(Item::new("world", "World", "text/html", world))?;
    c.add_redirection("hi", "Hi", "hello", false)?;
    c.finish()?;
    println!("wrote {out}");
    Ok(())
}
