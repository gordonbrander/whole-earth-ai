# Whole Earth AI — PRD

## TLDR

Two deliverables from one repo. **`zim-mcp`** is a standalone Rust CLI, published on crates.io and as prebuilt binaries, that gives any AI tool offline access to Kiwix ZIM archives such as Wikipedia. It does three independent jobs: it **serves** archives over MCP (stdio), using Kiwix's own libzim for reading and full-text search; it **manages** the library (browse the Kiwix catalog, download, verify, update), from the command line or from inside a chat, where every change needs the user's approval; and it **exports** articles as Markdown. It works with whatever AI tool you already use (Claude Code, Pi, Claude Desktop, LM Studio…) and needs no local model. The **Whole Earth drive** is an optional bundle built on top of it: a drive you plug into any Mac, Windows or Linux computer that holds a full offline copy of Wikipedia, `zim-mcp`, a small local model and llamafile to run it. You double-click a launcher, a chat page opens in your browser, and the model answers from the offline library, citing the articles it read, with no network and no install. The bundle is all existing software plus launchers and a justfile that assembles the drive from a pinned manifest. `zim-mcp` knows nothing about llamafile; the bundle only configures it.

## User stories

- **S1** As a person with unreliable or no internet (rural, travelling, disaster, censored), I want to plug in a drive and ask questions in plain language, so I can get answers based on Wikipedia without a network.
- **S2** As a privacy-minded person, I want both the AI and the knowledge on hardware I hold, so I can ask anything without it leaving my machine.
- **S3** When the assistant answers, I want to see which articles it read, so I can check its claims against the source.
- **S4** When I already use an MCP-capable AI app (Claude Desktop, LM Studio, Jan, etc.), I want to point it at the drive's archives, so I can use my preferred AI with the offline library.
- **S5** When I'm back online, I want to add an archive (Wiktionary, Wikivoyage, Stack Exchange…) or get the newest Wikipedia with one command, so the drive stays current without any other tools.
- **S6** When a multi-hour download is interrupted or a file is damaged, I want it to resume and never leave a broken archive in the library, so I can trust what's on the drive.
- **S7** As the maintainer, I want to rebuild a drive from a pinned manifest with one command, so every drive is reproducible and auditable.
- **S8** As the maintainer, I want CI to build `zim-mcp` for every platform on release, so I don't need a Mac, a Windows PC and a Linux box to ship.
- **S9** As a developer with my own ZIM files and AI setup, I want to `cargo install zim-mcp`, so I can use the server and library manager without the drive.
- **S10** When I want to use ZIM content outside Kiwix (a notes app, grep, a static site, my own indexing pipeline), I want to export one article, a list of articles or a whole archive as Markdown files, so the knowledge isn't locked inside one file format.
- **S11** As a Claude Code or Pi user, I want to download only the `zim-mcp` binary and register it with my agent, so my agent can search offline Wikipedia without a local model or the drive.
- **S12** When I'm chatting with my AI, I want to ask it to find and download an archive (or update or remove one), and approve the change myself, so I can manage the library without leaving the conversation and without the AI acting on its own.

## Goals

- Someone who isn't technical goes from plugging in the drive to an answer drawn from the library in about 2 minutes on a typical laptop, with no install and no network.
- A small local model answers noticeably more accurately with the library than without it, measured on a fixed question set we keep in the repo.
- One drive works on macOS (Apple Silicon and Intel), Windows x64, and Linux x64/arm64.
- Any ZIM from the Kiwix library works as soon as it's on the drive, with no import or indexing step, and search finds the same results the Kiwix apps would.
- Any drive can be rebuilt from the repo with the same contents.
- Archives on the drive can be kept current in place: an update replaces the old version.
- `zim-mcp` stands alone: no command needs llama.cpp, llamafile or a model, and it works with any MCP client. The drive is one way to use it, not a requirement.
- Content can leave the archive: any article can become a readable, attributed Markdown file.

### Non-goals

- Our own model runtime, chat UI or agent loop. llamafile/llama.cpp provides these.
- A terminal chat interface in v1 (listed as a future MAY under Requirements).
- Our own ZIM reader or search index (pure-Rust Xapian reader, Tantivy, embeddings, semantic search). libzim provides reading and search.
- An article viewer with images for people to browse (that's Kiwix / kiwix-serve).
- Bootable OS images, Docker images, installers, or a self-updater for binaries.
- Torrent downloads, or downloading from several mirrors in parallel.
- Creating or editing ZIM files.
- Exporting to formats other than Markdown (HTML, PDF, EPUB). `zimdump` from zim-tools already dumps raw HTML.
- Letting the model change the library without the user's explicit approval for that specific change.
- Network-exposed or multi-user deployments. Everything stays on localhost.
- Choosing or tuning a model beyond picking one default from an evaluation.
- Binaries with a permissive license. `zim-mcp` is GPL-3.0-or-later throughout.

## Requirements

### Serving (`zim-mcp serve`)

- MUST serve all ZIM archives in a given directory over MCP using the stdio transport. (S1, S4, S9)
- MUST speak both the stateless 2026-07-28 MCP protocol and the older `initialize` handshake. llama.cpp's stdio client, and so llamafile 0.10.6, still sends `initialize` at 2024-11-05. (S1, S4, S11)
- MUST offer a full-text search tool covering every archive, and a tool that reads an article by ID. (S1, S4)
- MUST include, in every search result and article read, the archive name, article title and a stable article ID the model can quote. (S3)
- MUST keep tool output small enough for a small model's context: short snippets, and long articles split into pages with a way to ask for the next page. (S1)
- MUST work with unmodified Kiwix ZIMs with no import or indexing step, and fall back to title suggestions for archives without a full-text index. (S1, S5)
- MUST keep the search, read and list tools read-only and offline. Only the library tools may use the network or change files. (S2, S4)
- MUST start up and answer one tool call fast enough that llama-server starting `zim-mcp` fresh for each call feels instant. Target: under 500 ms with the 119 GiB maxi archive on a USB SSD. (S1)
- MUST ignore files that are mid-download (`*.part`). (S6)
- SHOULD return Markdown converted from the article's HTML, using the same conversion as export, so headings and lists survive. (S1, S10)
- SHOULD offer a tool that lists the archives with their name, language, date and article count. (S1, S4)
- SHOULD let the model read a single section of an article, or its outline. (S1)
- MAY offer title suggestions and a tool that lists an article's links. (S1)
- SHOULD offer the same operations as plain CLI commands (`zim-mcp search`, `zim-mcp read`, `zim-mcp list`) printing Markdown, or JSON with `--json`, for agents and scripts that prefer CLI tools to MCP. (S11)

### Library (`zim-mcp library` and MCP)

- MUST search the Kiwix online catalog by name, language and category. (S5)
- MUST add an archive by catalog name (newest version) or by a direct URL. (S5)
- MUST check the sha256 published by Kiwix before an archive appears in the library. Partial or unverified files must never sit where `zim-mcp serve` would read them. (S6)
- MUST resume interrupted downloads. (S6)
- MUST make `update` replace each archive with its newest catalog version, removing the old file only after the new one's checksum checks out. (S5, S6)
- MUST list and remove archives on the drive. (S5)
- SHOULD show download progress and an estimated time remaining. (S5)
- MAY try the other mirrors listed in the metalink when one fails. (S6)
- MUST offer the library operations over MCP as well as the CLI: catalog search, list and status without approval; add, update and remove only with approval. (S12)
- MUST get the user's explicit approval for every add, update and remove requested over MCP, through MCP elicitation, showing the archive, its download size and anything that will be deleted. The model can never approve on the user's behalf. (S2, S12)
- MUST refuse a change when the client can't ask the user, and return the equivalent CLI command instead, unless the user started the server with `--trust-client-approval` (meaning: rely on the client's own per-call permission prompt). (S2, S12)
- MUST run approved downloads in the background and return immediately, because MCP tool calls time out (llama.cpp's default is 30 s) and a download can take hours; offer a status tool reporting progress. (S12)
- SHOULD mark every tool with MCP annotations (`readOnlyHint`, `destructiveHint`, `openWorldHint`) so clients can apply their own permission prompts too. (S12)
- SHOULD let the library tools be hidden at serve time (`--no-library`), because small models do worse with longer tool lists. (S1, S12)

### Export (`zim-mcp export`)

- MUST export one or more entries (by path) or every article in an archive to Markdown files in a chosen directory. (S10)
- MUST start each file with front matter giving the title, archive, entry path, archive date and a CC BY-SA attribution line where the archive's metadata states that license. (S3, S10)
- MUST skip non-article entries (images, CSS, scripts) and represent redirects without duplicating content. (S10)
- MUST never modify the archive. (S6, S10)
- SHOULD rewrite links between articles as relative links between the exported files, so the export can be browsed as a folder. (S10)
- SHOULD split a whole-archive export into subfolders, because English Wikipedia has millions of articles and a single folder would hit filesystem and tool limits. (S10)
- SHOULD let an interrupted whole-archive export resume, skipping files that already exist. (S10)
- MAY export the images an article references, alongside the Markdown. (S10)

### Distribution: standalone `zim-mcp`

- MUST NOT require llama.cpp, llamafile or a model for any `zim-mcp` command. Those belong only to the drive bundle. (S9, S11)
- MUST publish per-OS release archives on GitHub Releases containing `zim-mcp` and the libzim shared libraries it needs, for macOS (universal), Windows x64, and Linux x64/arm64 (glibc), so nothing else needs installing. (S8, S11)
- MUST publish the `zim-mcp` crate to crates.io under GPL-3.0-or-later, with only the Rust sources in the package (no packaging files), building against a libzim installed on the system. (S9)
- MUST meet GPL-3.0 terms for every binary we distribute: include the GPL text and a source offer pointing to the exact `zim-mcp` commit and libzim release. (S7, S9, S11)
- SHOULD build every release archive in CI when a release is tagged. (S8)
- SHOULD document one-line setup for common clients in the README (Claude Code, Pi, Claude Desktop, LM Studio). (S4, S11)
- SHOULD sign and notarize the binaries, including the libzim dylib, so macOS and Windows don't block or warn on first run. (S1, S11)

### Distribution: Whole Earth drive (optional bundle)

- MUST include one double-click launcher per OS that starts llamafile with the drive's model, sets up `zim-mcp` as its MCP server, and opens the chat UI in the default browser. (S1)
- MUST use the same `zim-mcp` release archives as the standalone distribution, unmodified. (S7, S11)
- MUST use paths relative to the drive, need no install, and write nothing outside the drive except temp files. (S1, S2)
- MUST listen on localhost only. (S2)
- MUST rebuild a drive with one command from a pinned manifest (URL + sha256 for llamafile, the model and the `zim-mcp` release; catalog names for the ZIMs). (S7)
- MUST include the licenses and attributions for everything else on the drive: Wikipedia CC BY-SA, the model's license, and llamafile's license. (S7)
- SHOULD tell the model to search before answering and to cite the articles it read. (S3)
- MAY fall back to running the binaries from a temp folder when Linux mounts the drive without permission to execute. (S1)

## Proposal

**Deliverables**

| Deliverable | Contents | How people get it |
|---|---|---|
| `zim-mcp` (required) | The Rust CLI plus libzim's shared libraries | `cargo install zim-mcp` (needs libzim installed), or a per-OS archive from GitHub Releases (nothing else needed) |
| Whole Earth drive (optional) | `zim-mcp` release archives + llamafile + model + ZIMs + launchers | Assembled with `just assemble <dest>` |

Using `zim-mcp` with your own tool:
```
claude mcp add wiki -- zim-mcp serve ~/zim             # Claude Code
zim-mcp search ~/zim "photosynthesis"                   # any agent or script that prefers CLI tools
```

**Components**

| Piece | What it is | Who makes it | Needed by |
|---|---|---|---|
| ZIM archives | Wikipedia etc., with a full-text Xapian index built in | Kiwix | both |
| libzim | C++ library for reading ZIM files and searching them (Xapian, zstd, xz, ICU built in). Prebuilt for each platform | Kiwix / openZIM | `zim-mcp` |
| `zim-mcp` | Rust CLI: `serve` (MCP), `search`/`read`/`list` (same operations as CLI commands), `library` (managing) and `export` (Markdown), linking libzim through a small C++ shim | Us | both |
| llamafile | One cross-platform executable: llama-server, chat web UI, stdio MCP client | Mozilla (llamafile), on llama.cpp | drive only |
| Model | One small GGUF model that's good at tool calling (~4–8B parameters, Q4) | Chosen by evaluation | drive only |
| Launchers | `Start (Mac).command`, `Start (Windows).bat`, `start-linux.sh` | Us | drive only |
| justfile + `bundle/manifest.sha256` | Assembles the drive | Us | drive only |

**Repo layout** (one repo, one crate at the root; everything drive-specific lives under `bundle/`)

```
whole-earth-ai/
  Cargo.toml        # package "zim-mcp", license "GPL-3.0-or-later", include = ["src/**", "README.md", "LICENSE"]
  src/              # main.rs, zim/ (cxx bridge + shim.cpp), markdown.rs, tools.rs, mcp.rs, cli.rs, export.rs, library/
  .github/workflows/release.yml   # builds zim-mcp release archives
  justfile          # build (zim-mcp), fetch + assemble (drive)
  bundle/
    manifest.sha256 # pinned llamafile, model, zim-mcp release
    launchers/
  PRD.md
```

**Drive layout (optional bundle; exFAT)**

```
WHOLE-EARTH/
  Start (Mac).command   Start (Windows).bat   start-linux.sh
  llamafile.exe                       # one file for every OS; .exe so Windows will run it
  bin/macos-universal/  zim-mcp, libzim.9.dylib
  bin/windows-x64/      zim-mcp.exe, zim-9.dll, icu*.dll
  bin/linux-x64/        zim-mcp, libzim.so.9
  bin/linux-arm64/      zim-mcp, libzim.so.9
  models/model.gguf                   # kept outside llamafile (Windows won't run executables over 4 GB)
  zim/*.zim                           # this folder *is* the library
  LICENSES/                           # GPL-3.0 + source offer, CC BY-SA, model, llamafile
```

**Runtime flow.** The launcher detects the OS and CPU type, then runs llamafile in server mode on `127.0.0.1` with `-m models/model.gguf` and `--mcp-servers-json '{"mcpServers":{"wiki":{"command":"<drive>/bin/<os>/zim-mcp","args":["serve","<drive>/zim"]}}}'`, and opens `http://127.0.0.1:8080`. llama-server starts `zim-mcp serve` once to list its tools, then again for each tool call. The model sees `wiki_search`, `wiki_read`, `wiki_list_archives`. Whether the drive exposes the library tools depends on the llama.cpp approval question under Open questions. Until then the launcher passes `--no-library`, and drive users manage the library with the CLI.

**`zim-mcp serve` tools** (kept small, because small models misuse long tool lists):
- `search(query, archive?, limit=5)` → for each hit: `{archive, title, id, snippet}`. Uses libzim's full-text search, which searches several archives together when they share a language. Archives without a full-text index fall back to libzim's title suggestions.
- `read(id, page=1)` → Markdown of about 2–4k characters (the same conversion as export), with `{title, archive, id, page, pages}`.
- `list_archives()` → `{name, title, language, date, articles, has_fulltext}` from the ZIM's metadata.

Article ID format: `<archive-name>/<entry-path>`, e.g. `wikipedia_en_all_maxi/A/Albert_Einstein`. The MCP server uses `rmcp`, the official Rust MCP SDK.

**libzim binding.** A thin C++ shim exposed through the `cxx` crate, covering only what the tools need: open an archive, read metadata, look up an entry by path, read item content, full-text search, title suggestions. Roughly 200–400 lines.

**`zim-mcp library`**

```
zim-mcp library search <query> [--lang eng] [--category wikipedia]
zim-mcp library add <name|url> [--dir zim/]
zim-mcp library list
zim-mcp library update
zim-mcp library remove <name>
```

- The catalog is Kiwix's OPDS feed (`library.kiwix.org/catalog/v2/entries`). Each entry links to a `.meta4` metalink file giving the exact size, the sha256 and a list of mirrors.
- Download sequence: download to `name_YYYY-MM.zim.part` with HTTP Range resume, computing the sha256 while streaming, check it, then rename into place. `update` does this for each archive with a newer catalog date, then deletes the old file.
- There's no database. The `zim/` folder is the library, and the version comes from the filename (`<name>_<YYYY-MM>.zim`).
- Over MCP, the same operations are tools: `library_search`, `library_list`, `library_status` (no approval), and `library_add`, `library_update`, `library_remove` (approval required).
- **Approval:** each change tool sends an `elicitation/create` confirmation ("Download wikipedia_en_all_maxi (119 GiB) to zim/? This replaces wikipedia_en_all_maxi_2026-02.zim.") and acts only on the user's "accept". If the client doesn't support elicitation, the tool returns an error with the CLI command to run, unless `--trust-client-approval` was given.
- **Background downloads:** an approved add/update starts a detached `zim-mcp library add …` process and returns at once. That process is the same code path as the CLI. It saves the `.meta4` beside the `.part` file, so `library_status` reads progress straight from the files (`.part` size vs. size in `.meta4`). No daemon and no state outside the `zim/` folder.

**`zim-mcp export`**

```
zim-mcp export <archive.zim> <entry-path>... --out <dir>   # selected articles
zim-mcp export <archive.zim> --all --out <dir>             # whole archive
```

- Output: `<dir>/<archive-name>/<shard>/<entry-path>.md`, where `<shard>` is the first two characters of the entry path, so no folder gets millions of files. Single-entry exports go in the same layout, so links between them keep working.
- Each file starts with YAML front matter (`title`, `archive`, `path`, `date`, `source`, `license`), followed by the Markdown body.
- HTML → Markdown with `htmd`, the same converter `read` uses. Links to other articles become relative `.md` links; links outside the archive stay as they are. Redirects become a short file pointing to the target article.
- CLI only, never an MCP tool (it writes files).

**Crates.** `rmcp` (+ `tokio`; the official Rust MCP SDK, which negotiates every protocol version from 2024-11-05 to the stateless 2026-07-28 over stdio, so moving to the new protocol is a version bump), `cxx`/`cxx-build`, `ureq` (downloads), `sha2`, `quick-xml` (OPDS and `.meta4`), `htmd` (HTML → Markdown; front matter is a few quoted strings, written by hand), `clap`, `indicatif`, `serde_json`.

**Build and assembly**
- `build.rs`: if `LIBZIM_DIR` is set, use that prebuilt libzim; otherwise find a system libzim with pkg-config (crates.io users: `brew install libzim`, `apt install libzim-dev`). Compile the shim with `cxx_build`, link `-lzim`, and make `zim-mcp` look for libzim next to itself (`@executable_path` on macOS, `$ORIGIN` on Linux; Windows already does).
- CI (`release.yml`): a matrix on native runners (macOS arm64, macOS Intel, Windows x64, Linux x64, Linux arm64). Each job downloads its pinned libzim from `download.openzim.org/release/libzim/`, checks the sha256, runs `cargo build --release`, and packages `zim-mcp` plus its libraries, the GPL text and a source-offer note into `zim-mcp-<version>-<target>.tar.gz` (`.zip` on Windows). A final job merges the macOS binaries and dylibs with `lipo` and publishes the archives and their sha256 sums to GitHub Releases. This is the whole standalone deliverable.
- `just fetch` (drive only): downloads llamafile, the model and the `zim-mcp` release archives listed in `bundle/manifest.sha256` and checks them with `shasum -a 256 -c`.
- `just assemble <dest>` (drive only): lays out the folders, unpacks the `zim-mcp` release archives into `bin/<os>/`, copies launchers and licenses, and runs `zim-mcp library add` for each ZIM in the manifest.

**Code shape.** One crate, one binary, these modules: `zim` (the libzim binding), `markdown` (HTML → Markdown and link rewriting, pure functions), `tools` (search, read, list, built on `zim` + `markdown`), `mcp` and `cli` (two thin front ends exposing the same `tools` and `library` operations; `mcp` adds the approval step), `export` (built on `zim` + `markdown`), and `library` (catalog and downloads). `library` doesn't use the others; it only shares the archive folder convention and the filename parser. Nothing in the crate refers to llamafile or the drive.

**Licensing.** `zim-mcp` source and binaries: GPL-3.0-or-later (libzim's GPL-2.0-or-later allows this; GPL-3 also accepts the Apache-2.0 Rust dependencies). Binaries ship with the license text and a source offer. llamafile, the model and the ZIM content are separate programs and data, connected over stdio MCP, so libzim's license doesn't extend to them.

## Rationale and alternatives

The proposal uses existing software for everything except the one missing piece: a small, cross-platform ZIM tool server that works well with small models, plus a way to keep the archives current. libzim is the same code the Kiwix apps use, so reading and search behave the same and are maintained upstream. We accept a C++ dependency and GPL binaries in exchange for not writing a search engine reader. Each archive is its own source of truth (index included), the `zim/` folder is the library's state, llamafile owns the model and conversation, and `zim-mcp` only translates between them. The drive only configures `zim-mcp` through llamafile's MCP settings and doesn't depend on it in any other way, so `zim-mcp` works the same anywhere else and the drive can switch to another runtime without changing it.

**Do nothing: users put it together themselves (Kiwix + LM Studio/Jan + openzim-mcp).** All the pieces exist. But it needs several installs, a Python runtime and hand-written MCP configuration, and it doesn't run from a drive. It ties the user's ability to get answers to their ability to set up software, which is exactly the barrier S1 removes.

**Most boring existing option: ship zimz-mcp unchanged with llamafile and a script.** Close in spirit, but zimz isn't on crates.io (blocking S9), doesn't cover the library stories (S5, S6), and its 8 tools may be too many for a 4–8B model.

**Pure Rust: the `zim` crate plus our own read-only Xapian "glass" reader.** Allows building every target from one machine, a static Linux binary, and `cargo install` with no system libraries. But the Xapian reader is several thousand lines of reverse-engineered format code that has to match libzim's stemming and ranking. That's the largest risk in the project, and libzim already does it. Our shim sits behind the MCP tools, so a pure-Rust reader could replace it later without changing the tool interface.

**Rust Xapian wrappers (`xapian-rs`, `xapian-rusty`) plus a ZIM reader.** Still a C++ dependency, plus extracting the index from the ZIM ourselves. libzim already combines both.

**Split into a GPL MCP server and a permissive library manager talking over MCP.** Legally clean (separate programs), and it would give a `cargo install` that needs no libzim. It costs a second crate and binary per OS, and since everything is GPL-3 anyway it buys little. The `library` module doesn't depend on `zim`, so splitting later is easy.

**Build our own index (Tantivy + embeddings), as offline-knowledge does.** Better control over ranking, and room for semantic search. But it ties the archive to separately derived state: an import step (hours for maxi), tens of GB of extra disk, and an index that must be rebuilt and kept consistent on every update. It breaks "any ZIM works as soon as it's on the drive".

**Our own agent loop and terminal chat (`zim-mcp chat`).** Gives full control over prompts and trimming context. But it ties retrieval to orchestration and UI, which llamafile already provides. llamafile's terminal chat and agentfile can't use MCP, so a terminal interface means this loop. It's deferred to a future MAY, and if built, it should be its own module calling the same ZIM functions, not woven into `mcp`.

**Library over CLI only.** Simplest and safest, but forces users out of the conversation to grow the library (S12).

**Two-step confirm in the tool itself** (the tool returns a plan and a token; a second call with the token goes ahead). No client support needed, but the model can make the second call itself, so it isn't the user's approval.

**Rely on each client's own tool-permission prompt.** Claude Code and others prompt per call, but not every client does, and auto-approve settings would make downloads silent. Kept only as the explicit `--trust-client-approval` opt-in.

**Export with existing tools: `zimdump` + pandoc.** The most boring route to Markdown. But it's two more tools to install, it writes millions of intermediate HTML files, it doesn't rewrite links or add attribution front matter, and its output wouldn't match what the model sees through `read`. Using one converter for both keeps "what the AI read" and "what you exported" the same text.

**Hand-written JSON-RPC instead of `rmcp`.** About 200 lines with no tokio, but we would have to track every spec change and keep both the stateless and `initialize` paths working ourselves. Worth reconsidering only if `rmcp` adds noticeably to build time or binary size.

**MCP over HTTP + the web UI's CORS proxy.** Needed before llama.cpp could launch stdio servers. It ties serving to network setup (ports, CORS, a manual connection step in the UI). Now unnecessary.

**kiwix-serve + an HTTP-to-MCP wrapper.** Two runtimes, and parsing HTML search pages. More moving parts than calling libzim directly.

**Official llama.cpp builds per OS instead of llamafile.** The fallback if llamafile's sandbox blocks starting `zim-mcp`, or it lags too far behind upstream. It ties our packaging to GPU backend libraries (Metal/Vulkan/CUDA DLLs) for each OS. Only the launchers would change.

**A registry or database for the library.** A second record of what's on the drive, which can drift from the files. The directory already is that record.

**Bootable Linux image / Docker image.** Fully self-contained, but it doesn't boot on Apple Silicon, Docker on a Mac has no GPU access, and both ask more of users than a double-click.

## Prior art

- Whole Earth Catalog: "access to tools", the framing.
- Kiwix apps, kiwix-serve, libzim / libkiwix: https://kiwix.org, https://github.com/openzim/libzim, https://github.com/kiwix/kiwix-tools
- Prebuilt libzim for each platform: https://download.openzim.org/release/libzim/
- `zimdump` (zim-tools): dumps a ZIM's entries as raw files (HTML, images): https://github.com/openzim/zim-tools
- `htmd`: turndown.js-style HTML → Markdown in Rust: https://crates.io/crates/htmd
- Kiwix OPDS catalog: https://library.kiwix.org/catalog/v2/entries ; metalink downloads with sha256: https://download.kiwix.org/zim/
- Internet-in-a-Box: an offline knowledge server for schools and clinics: https://internet-in-a-box.org
- zimz: pure-Rust ZIM + Xapian reader with an MCP server: https://github.com/selfup/zimz
- offline-knowledge: Rust ZIM reader with Tantivy index, terminal UI, web UI and MCP: https://github.com/kohlhofer/offline-knowledge
- `zim` crate (pure-Rust reader, formats 5.0–6.3): https://github.com/dignifiedquire/zim
- openzim-mcp (Python): https://github.com/cameronrye/openzim-mcp
- Zimi (Python, catalog downloads + MCP): https://github.com/epheterson/zimi
- kiwix-mcp wrappers around kiwix-serve: https://github.com/OscillateLabsLLC/kiwix-mcp, https://github.com/roanpy/kiwix-mcp
- llama.cpp stdio MCP servers in llama-server: https://github.com/ggml-org/llama.cpp/pull/26062
- llamafile: https://github.com/mozilla-ai/llamafile
- `rmcp`, the official Rust MCP SDK: https://github.com/modelcontextprotocol/rust-sdk
- MCP 2026-07-28 (stateless) spec: https://modelcontextprotocol.io/specification/2026-07-28
- GPL FAQ on linking vs. separate programs: https://www.gnu.org/licenses/gpl-faq.html#MereAggregation

## Open questions

- **Does llamafile's sandbox allow llama-server to start `zim-mcp`?** Its notes say MCP fails silently if subprocess support is missing. If it's blocked, use official llama.cpp builds.
- **Is starting `zim-mcp` per call fast enough** with the 119 GiB maxi archive on USB, now that libzim also opens the Xapian index? If not, would llama.cpp keep the process alive, or does `zim-mcp` need a lighter startup path?
- **How much of libzim's C++ API does the shim need?** Does the `cxx` crate handle libzim's iterators and exceptions cleanly, or does a small C interface (`extern "C"`) plus `bindgen` work better?
- **Linux compatibility:** what's the oldest glibc the manylinux libzim build supports, and is that old enough for the machines we expect?
- **Intel Macs:** GitHub's Intel macOS runners are being phased out. Build the x86_64 slice by cross-compiling on arm64 (the prebuilt libzim already exists for x86_64), or drop Intel?
- **Licensing check:** confirm the GPL-3.0 approach and source-offer wording with someone qualified before any commercial drive sales.
- **Can the system prompt and MCP tool settings be preset from the server side,** so the first conversation already searches and cites without the user changing settings?
- **Which default model?** Run the question set against several 4–8B tool-calling models, and check each license allows redistribution.
- **Which default bundle?** Maxi (256 GB drive), nopic (128 GB) or mini (32 GB). One version of the drive or several?
- **Signing:** are an Apple Developer ID ($99/yr) and a Windows code-signing certificate worth it for v1?
- **Which archives besides English Wikipedia ship by default** (Wiktionary, Wikivoyage, WikiMed, Stack Exchange)? What about other languages?
- **Markdown quality for Wikipedia's harder parts:** infoboxes, complex tables, math (MathML/SVG), footnotes and navboxes. Strip some of these before conversion, and should `read` and `export` strip the same things?
- **Pi and MCP:** secondary sources say Pi added a native MCP client in 1.0 (earlier versions deliberately had none). Confirm against the official docs; the CLI commands cover Pi either way.
- **Elicitation support across clients:** do Claude Code, Pi, Claude Desktop and LM Studio support MCP elicitation? llama-server's stdio MCP code doesn't appear to. Does its web UI ask permission before calling MCP tools, which would justify `--trust-client-approval` on the drive?
- **How does the chat UI show citations:** does the web UI show tool results clearly enough for S3, or do we need a citation format in the system prompt?
