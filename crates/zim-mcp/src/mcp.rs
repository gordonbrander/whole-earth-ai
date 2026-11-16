//! MCP front end (stdio). Exposes `tools` (read-only, offline), the
//! `library` operations and `export`; anything that writes files needs the
//! user's approval through MCP elicitation.

use crate::library::{self, download};
use crate::{export, tools};
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    handler::server::{
        router::tool::ToolRouter,
        tool::{InputResponses, RequestState},
        wrapper::Parameters,
    },
    model::*,
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Server {
    dir: PathBuf,
    trust_client_approval: bool,
    tool_router: ToolRouter<Server>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// What to search for.
    query: String,
    /// Only search this archive (a name from list_archives).
    archive: Option<String>,
    /// Number of results (default 5).
    limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Article id from search, e.g. `wikipedia_en_all_maxi/Albert_Einstein`.
    id: String,
    /// Page of a long article (default 1).
    page: Option<usize>,
    /// Read only this section (a heading from the article's sections list).
    section: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CatalogArgs {
    /// Words to search the Kiwix catalog for, e.g. `wikivoyage`.
    query: Option<String>,
    /// ISO 639-3 language code, e.g. `eng`.
    lang: Option<String>,
    /// Category, e.g. `wikipedia`, `wiktionary`, `stack_exchange`.
    category: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct NameArgs {
    /// Archive name from library_search or library_list, e.g. `wikipedia_en_all_maxi`.
    name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateArgs {
    /// Archives to update; all of them if empty.
    #[serde(default)]
    names: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ExportArgs {
    /// Article ids from search, e.g. `wikipedia_en_all_maxi/Albert_Einstein`.
    ids: Vec<String>,
    /// Absolute path of the folder to write the Markdown files into.
    out: String,
}

fn text(s: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(s.into())])
}

fn fail(s: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(s.into())])
}

fn done(r: Result<String, String>) -> CallToolResult {
    r.map_or_else(fail, text)
}

/// Run blocking work (archive reads, indexing, network) off the async runtime.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f)
        .await
        .expect("worker panicked")
}

enum Approval {
    Approved,
    /// Ask the user (2026-07-28 and later): the client retries with the answer.
    Ask(InputRequiredResult),
    Refused(String),
}

const CONFIRM: &str = "confirm";

impl Server {
    /// The user's approval for `message`. The model never answers this: the
    /// client shows the question to the user, and only an explicit "accept"
    /// lets the change happen.
    async fn approve(
        &self,
        ctx: &RequestContext<RoleServer>,
        message: &str,
        cli: &str,
        state: Option<String>,
        responses: Option<rmcp::model::InputResponses>,
    ) -> Approval {
        if self.trust_client_approval {
            return Approval::Approved;
        }
        let can_ask = ctx
            .client_capabilities()
            .is_some_and(|c| c.elicitation.is_some());
        if !can_ask {
            return Approval::Refused(format!(
                "This change needs the user's approval, but this app can't ask them. \
                 The user can run it themselves:\n\n    {cli}"
            ));
        }
        let declined = || Approval::Refused("The user declined. Nothing was changed.".into());
        let schema: ElicitationSchema =
            serde_json::from_value(serde_json::json!({"type": "object", "properties": {}}))
                .unwrap();
        let stateless = ctx.protocol_version().is_some_and(|v| !v.has_initialize());
        if stateless {
            // The request state is the exact question, so the answer only counts for it.
            if state.as_deref() == Some(message) {
                let accepted = responses
                    .as_ref()
                    .and_then(|r| r.get(CONFIRM))
                    .is_some_and(|r| r["action"] == "accept");
                return if accepted {
                    Approval::Approved
                } else {
                    declined()
                };
            }
            let mut requests = InputRequests::new();
            requests.insert(
                CONFIRM.to_string(),
                InputRequest::Elicitation(ElicitRequest::new(
                    ElicitRequestParams::FormElicitationParams {
                        meta: None,
                        message: message.to_string(),
                        requested_schema: schema,
                    },
                )),
            );
            return Approval::Ask(InputRequiredResult::new(
                Some(requests),
                Some(message.to_string()),
            ));
        }
        let params = ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: message.to_string(),
            requested_schema: schema,
        };
        match ctx.peer.create_elicitation(params).await {
            Ok(r) if r.action == ElicitationAction::Accept => Approval::Approved,
            Ok(_) => declined(),
            Err(e) => Approval::Refused(format!(
                "Couldn't ask the user ({e}). They can run it themselves:\n\n    {cli}"
            )),
        }
    }

    fn cli(&self, args: &str) -> String {
        format!(
            "zim-mcp library {args} --dir {:?}",
            self.dir.display().to_string()
        )
    }
}

#[tool_router(router = wiki_router)]
impl Server {
    #[tool(
        description = "Search the offline library (Wikipedia etc.). Returns article titles, ids and snippets. Read an article with `read` before answering, and cite the ids you used.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn search(&self, Parameters(a): Parameters<SearchArgs>) -> CallToolResult {
        let dir = self.dir.clone();
        done(
            blocking(move || {
                tools::search(&dir, &a.query, a.archive.as_deref(), a.limit.unwrap_or(5))
                    .map(|h| tools::hits_md(&h))
            })
            .await,
        )
    }

    #[tool(
        description = "Read an article by id (from search) as Markdown. Long articles come in pages; the first page lists the sections, and `section` reads just one.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn read(&self, Parameters(a): Parameters<ReadArgs>) -> CallToolResult {
        let dir = self.dir.clone();
        done(
            blocking(move || {
                tools::read(&dir, &a.id, a.page.unwrap_or(1), a.section.as_deref())
                    .map(|r| tools::article_md(&r))
            })
            .await,
        )
    }

    #[tool(
        description = "List the archives in the offline library: name, title, language, date, article count.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_archives(&self) -> CallToolResult {
        let dir = self.dir.clone();
        done(blocking(move || tools::list_archives(&dir).map(|l| tools::archives_md(&l))).await)
    }
}

#[tool_router(router = library_router)]
impl Server {
    #[tool(
        description = "Search the online Kiwix catalog for archives to download (needs internet).",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn library_search(&self, Parameters(a): Parameters<CatalogArgs>) -> CallToolResult {
        done(
            blocking(move || {
                library::catalog::search(
                    a.query.as_deref().unwrap_or(""),
                    a.lang.as_deref().unwrap_or(""),
                    a.category.as_deref().unwrap_or(""),
                    20,
                )
                .map(|e| crate::cli::catalog_md(&e))
            })
            .await,
        )
    }

    #[tool(
        description = "List the archive files in the library folder.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn library_list(&self) -> CallToolResult {
        let dir = self.dir.clone();
        done(crate::cli::local_md(&dir))
    }

    #[tool(
        description = "Show downloads in progress.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn library_status(&self) -> CallToolResult {
        done(download::status(&self.dir).map(|s| download::status_md(&s)))
    }

    #[tool(
        description = "Download an archive (newest version) into the library. Asks the user to approve first, then downloads in the background; check progress with library_status.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn library_add(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(a): Parameters<NameArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> Result<CallToolResponse, ErrorData> {
        let dir = self.dir.clone();
        let name = a.name.clone();
        let plan = match blocking(move || download::plan(&dir, &name)).await {
            Ok(p) => p,
            Err(e) => return Ok(fail(e).into()),
        };
        if plan.already_present {
            return Ok(text(plan.describe(&self.dir)).into());
        }
        self.run_approved(
            &ctx,
            vec![plan],
            &format!("add {}", a.name),
            state,
            responses,
        )
        .await
    }

    #[tool(
        description = "Replace archives with their newest catalog versions (all, or the named ones). Asks the user to approve first; the old file is deleted only after the new one is verified.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    async fn library_update(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(a): Parameters<UpdateArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> Result<CallToolResponse, ErrorData> {
        let dir = self.dir.clone();
        let names = a.names.clone();
        let plans = match blocking(move || download::plan_update(&dir, &names)).await {
            Ok(p) => p,
            Err(e) => return Ok(fail(e).into()),
        };
        if plans.is_empty() {
            return Ok(text("Everything is up to date.").into());
        }
        self.run_approved(
            &ctx,
            plans,
            &format!("update {}", a.names.join(" ")),
            state,
            responses,
        )
        .await
    }

    #[tool(
        description = "Delete an archive from the library. Asks the user to approve first.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn library_remove(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(a): Parameters<NameArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> Result<CallToolResponse, ErrorData> {
        let files: Vec<String> = library::scan(&self.dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.name == a.name)
            .map(|f| {
                let size = f.path.metadata().map_or(0, |m| m.len());
                format!(
                    "{} ({})",
                    f.path.file_name().unwrap().to_string_lossy(),
                    library::human_size(size)
                )
            })
            .collect();
        if files.is_empty() {
            return Ok(fail(format!("No archive named {:?}; see library_list.", a.name)).into());
        }
        let message = format!("Delete {} from {}?", files.join(", "), self.dir.display());
        match self
            .approve(
                &ctx,
                &message,
                &self.cli(&format!("remove {}", a.name)),
                state,
                responses,
            )
            .await
        {
            Approval::Approved => Ok(done(
                library::remove(&self.dir, &a.name)
                    .map(|f| format!("Deleted {} file(s).", f.len()))
                    .map_err(|e| e.to_string()),
            )
            .into()),
            Approval::Ask(r) => Ok(r.into()),
            Approval::Refused(why) => Ok(fail(why).into()),
        }
    }

    #[tool(
        description = "Save articles (ids from search) as Markdown files with attribution front matter in a folder. Asks the user to approve first.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn export(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(a): Parameters<ExportArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
    ) -> Result<CallToolResponse, ErrorData> {
        if a.ids.is_empty() {
            return Ok(fail("Give at least one article id.").into());
        }
        let out = PathBuf::from(&a.out);
        if !out.is_absolute() {
            return Ok(fail(format!("`out` must be an absolute path, not {:?}.", a.out)).into());
        }
        // Sorted, so the approval message (the request state) is the same on retry.
        let mut by_archive: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in &a.ids {
            match tools::parse_id(id) {
                Ok((name, path)) => by_archive
                    .entry(name.to_string())
                    .or_default()
                    .push(path.to_string()),
                Err(e) => return Ok(fail(e).into()),
            }
        }
        let newest = library::newest(&self.dir).unwrap_or_default();
        let mut jobs: Vec<(PathBuf, Vec<String>)> = vec![];
        for (name, paths) in by_archive {
            let Some(archive) = newest.iter().find(|x| x.name == name) else {
                return Ok(fail(format!("No archive named {name:?}; see list_archives.")).into());
            };
            jobs.push((archive.path.clone(), paths));
        }
        let message = format!(
            "Export {} article(s) as Markdown to {}?\n{}",
            a.ids.len(),
            out.display(),
            jobs.iter()
                .map(|(f, p)| format!(
                    "{}: {}",
                    f.file_name().unwrap().to_string_lossy(),
                    p.join(", ")
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let cli = jobs
            .iter()
            .map(|(f, p)| {
                format!(
                    "zim-mcp export {:?} {} --out {:?}",
                    f.display().to_string(),
                    p.iter()
                        .map(|p| format!("{p:?}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    out.display().to_string()
                )
            })
            .collect::<Vec<_>>()
            .join("\n    ");
        match self.approve(&ctx, &message, &cli, state, responses).await {
            Approval::Approved => Ok(done(blocking(move || run_export(&jobs, &out)).await).into()),
            Approval::Ask(r) => Ok(r.into()),
            Approval::Refused(why) => Ok(fail(why).into()),
        }
    }
}

fn run_export(jobs: &[(PathBuf, Vec<String>)], out: &Path) -> Result<String, String> {
    let mut s = export::Stats::default();
    for (archive, paths) in jobs {
        let t = export::export(archive, paths, false, out, false)?;
        s.written += t.written;
        s.redirects += t.redirects;
        s.skipped_existing += t.skipped_existing;
        s.skipped_non_article += t.skipped_non_article;
    }
    Ok(format!(
        "Exported to {}: {} articles, {} redirects written; {} already existed; {} non-articles skipped.",
        out.display(),
        s.written,
        s.redirects,
        s.skipped_existing,
        s.skipped_non_article
    ))
}

impl Server {
    async fn run_approved(
        &self,
        ctx: &RequestContext<RoleServer>,
        plans: Vec<download::Plan>,
        cli_args: &str,
        state: Option<String>,
        responses: Option<rmcp::model::InputResponses>,
    ) -> Result<CallToolResponse, ErrorData> {
        let message = plans
            .iter()
            .map(|p| p.describe(&self.dir))
            .collect::<Vec<_>>()
            .join("\n");
        match self
            .approve(ctx, &message, &self.cli(cli_args.trim()), state, responses)
            .await
        {
            Approval::Approved => {
                for p in &plans {
                    if let Err(e) = download::spawn_background(&self.dir, p) {
                        return Ok(fail(e).into());
                    }
                }
                let files: Vec<&str> = plans.iter().map(|p| p.file.as_str()).collect();
                Ok(text(format!(
                    "Started downloading {} in the background. Check progress with library_status.",
                    files.join(", ")
                ))
                .into())
            }
            Approval::Ask(r) => Ok(r.into()),
            Approval::Refused(why) => Ok(fail(why).into()),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = Implementation::new("zim-mcp", env!("CARGO_PKG_VERSION"));
        info.instructions = Some(
            "Offline library of reference archives (e.g. Wikipedia). For any factual question, always \
             search the library first, even if you think you know the answer, then read the most \
             relevant article and answer from it. Cite the article ids you used."
                .into(),
        );
        info
    }
}

pub fn serve(dir: PathBuf, no_library: bool, trust_client_approval: bool) -> Result<(), String> {
    let mut router = Server::wiki_router();
    if !no_library {
        router += Server::library_router();
    }
    let server = Server {
        dir,
        trust_client_approval,
        tool_router: router,
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let running = server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| e.to_string())?;
        running.waiting().await.map_err(|e| e.to_string())?;
        Ok(())
    })
}
