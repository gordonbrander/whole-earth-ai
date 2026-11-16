//! Command-line front end: the same `tools` and `library` operations as the
//! MCP server, printing Markdown (or JSON with `--json`).

use crate::library::{self, catalog, download};
use crate::{export, mcp, tools};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "zim-mcp",
    version,
    about = "Offline Kiwix ZIM archives (Wikipedia etc.) for any AI tool"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the archives in DIR over MCP (stdio).
    Serve {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// Hide the library tools (fewer tools suit small models).
        #[arg(long)]
        no_library: bool,
        /// Don't ask for approval over MCP; rely on the client's own per-call permission prompt.
        #[arg(long)]
        trust_client_approval: bool,
    },
    /// Full-text search across the archives in DIR.
    Search {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        query: String,
        #[arg(long)]
        archive: Option<String>,
        #[arg(long, default_value_t = 5)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Read an article by id (`<archive>/<path>`) as Markdown.
    Read {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        id: String,
        #[arg(long, default_value_t = 1)]
        page: usize,
        #[arg(long)]
        section: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List the archives in DIR.
    List {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Build the full-text search index of the archives in DIR that lack one
    /// (or only of NAME), in DIR's hidden `.zimfile/` folder, and remove the
    /// indexes of archives that are gone. `library add` indexes already.
    Index {
        name: Option<String>,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// Rebuild even if a valid index exists.
        #[arg(long)]
        force: bool,
    },
    /// Manage the library: browse the Kiwix catalog, download, update, remove.
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Export articles as Markdown files.
    Export {
        archive: PathBuf,
        /// Entry paths to export (or use --all).
        paths: Vec<String>,
        #[arg(long, conflicts_with = "paths")]
        all: bool,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum LibraryCommand {
    /// Search the online Kiwix catalog.
    Search {
        #[arg(default_value = "")]
        query: String,
        /// ISO 639-3 language code, e.g. eng.
        #[arg(long, default_value = "")]
        lang: String,
        #[arg(long, default_value = "")]
        category: String,
        #[arg(long, default_value_t = 20)]
        count: u32,
        #[arg(long)]
        json: bool,
    },
    /// Download archives by catalog name (newest version) or URL, verified against Kiwix's sha256.
    Add {
        #[arg(required = true)]
        specs: Vec<String>,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// Delete the archive's other versions once the new one is verified.
        #[arg(long)]
        replace: bool,
        /// Running in the background: remove `<file>.log` on success.
        #[arg(long, hide = true)]
        log: bool,
    },
    /// List the archive files.
    List {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Replace archives with their newest catalog versions.
    Update {
        names: Vec<String>,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Delete an archive (every version of it).
    Remove {
        name: String,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Show downloads in progress.
    Status {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
}

fn print(json: bool, value: impl serde::Serialize, md: impl FnOnce() -> String) {
    if json {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    } else {
        println!("{}", md());
    }
}

pub fn catalog_md(entries: &[catalog::CatalogEntry]) -> String {
    if entries.is_empty() {
        return "Nothing found in the catalog.".into();
    }
    entries
        .iter()
        .map(|e| {
            format!(
                "- **{}** ({}) {} · {} · {} · {}",
                e.name,
                e.date,
                e.title,
                e.language,
                library::human_size(e.size),
                e.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn local_md(dir: &Path) -> Result<String, String> {
    let list = library::scan(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if list.is_empty() {
        return Ok(format!("No archives in {}.", dir.display()));
    }
    Ok(list
        .iter()
        .map(|a| {
            let size = a.path.metadata().map_or(0, |m| m.len());
            let index = match zimfile::search::size(&a.path) {
                Some(n) => format!("index {}", library::human_size(n)),
                None => "not indexed".into(),
            };
            format!(
                "- **{}** {} · {} · {index} · {}",
                a.name,
                a.date,
                library::human_size(size),
                a.path.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn run() -> Result<(), String> {
    match Cli::parse().command {
        Command::Serve {
            dir,
            no_library,
            trust_client_approval,
        } => mcp::serve(dir, no_library, trust_client_approval),
        Command::Search {
            dir,
            query,
            archive,
            limit,
            json,
        } => {
            let hits = tools::search(&dir, &query, archive.as_deref(), limit)?;
            print(json, &hits, || tools::hits_md(&hits));
            Ok(())
        }
        Command::Read {
            dir,
            id,
            page,
            section,
            json,
        } => {
            let a = tools::read(&dir, &id, page, section.as_deref())?;
            print(json, &a, || tools::article_md(&a));
            Ok(())
        }
        Command::List { dir, json } => {
            let l = tools::list_archives(&dir)?;
            print(json, &l, || tools::archives_md(&l));
            Ok(())
        }
        Command::Export {
            archive,
            paths,
            all,
            out,
        } => {
            if paths.is_empty() && !all {
                return Err("give entry paths to export, or --all".into());
            }
            let s = export::export(&archive, &paths, all, &out, all)?;
            eprintln!(
                "{} articles, {} redirects written; {} already existed; {} non-articles skipped",
                s.written, s.redirects, s.skipped_existing, s.skipped_non_article
            );
            Ok(())
        }
        Command::Index { name, dir, force } => {
            let all = library::newest(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let chosen: Vec<_> = all
                .into_iter()
                .filter(|a| name.as_ref().is_none_or(|n| *n == a.name))
                .collect();
            if let Some(n) = &name
                && chosen.is_empty()
            {
                return Err(format!("no archive named {n:?} in {}", dir.display()));
            }
            let err = |e: zimfile::Error| format!("{}: {e}", dir.display());
            for a in chosen {
                if force {
                    zimfile::search::delete(&a.path).map_err(err)?;
                }
                library::index(&a.path, true)?;
            }
            for p in zimfile::search::prune(&dir).map_err(err)? {
                println!("removed {}", p.display());
            }
            Ok(())
        }
        Command::Library { command } => library_cmd(command),
    }
}

fn library_cmd(cmd: LibraryCommand) -> Result<(), String> {
    match cmd {
        LibraryCommand::Search {
            query,
            lang,
            category,
            count,
            json,
        } => {
            let e = catalog::search(&query, &lang, &category, count)?;
            print(json, &e, || catalog_md(&e));
        }
        LibraryCommand::Add {
            specs,
            dir,
            replace,
            log,
        } => {
            for spec in specs {
                let path = download::add(&dir, &spec, replace, !log)?;
                if log {
                    let _ = std::fs::remove_file(dir.join(format!(
                        "{}.log",
                        path.file_name().unwrap().to_string_lossy()
                    )));
                }
                println!("{}", path.display());
            }
        }
        LibraryCommand::List { dir } => {
            println!("{}", local_md(&dir)?);
            let s = download::status(&dir)?;
            if !s.is_empty() {
                println!("\nIn progress:\n{}", download::status_md(&s));
            }
        }
        LibraryCommand::Update { names, dir } => {
            let plans = download::plan_update(&dir, &names)?;
            if plans.is_empty() {
                println!("Everything is up to date.");
            }
            for p in plans {
                eprintln!("{}", p.describe(&dir).replace('?', "."));
                download::add(&dir, &p.meta4_url, true, true)?;
            }
        }
        LibraryCommand::Remove { name, dir } => {
            for f in library::remove(&dir, &name).map_err(|e| e.to_string())? {
                println!("removed {}", f.display());
            }
        }
        LibraryCommand::Status { dir } => {
            println!("{}", download::status_md(&download::status(&dir)?))
        }
    }
    Ok(())
}
