//! zim-mcp: offline access to Kiwix ZIM archives for any AI tool.
//! Serves them over MCP, manages the library, and exports Markdown.

mod cli;
mod export;
mod library;
mod markdown;
mod mcp;
mod tools;
mod zim;

fn main() {
    if let Err(e) = cli::run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
