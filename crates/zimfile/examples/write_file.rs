//! Write one file into an archive, streamed from disk:
//! `cargo run --release --example write_file -- big.bin out.zim`.

use std::path::PathBuf;
use zimfile::{Content, Creator, Item};

fn main() -> zimfile::Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let [input, out] = &args[..] else {
        eprintln!("usage: write_file <file> <out.zim>");
        std::process::exit(2);
    };
    let name = input.file_name().unwrap().to_string_lossy();
    let mut c = Creator::new(out);
    c.add_metadata("Title", name.as_ref(), "text/plain")?;
    c.add_item(Item::new(
        &name,
        &name,
        "application/octet-stream",
        Content::File(input.clone()),
    ))?;
    c.finish()
}
