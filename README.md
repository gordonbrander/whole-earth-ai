# zim-mcp

Offline access to [Kiwix](https://kiwix.org) ZIM archives (Wikipedia, Wiktionary,
Wikivoyage, Stack Exchange…) for any AI tool. One binary, three jobs:

- **serve** archives over MCP (stdio), with full-text search through Kiwix's own libzim;
- **manage** the library: browse the Kiwix catalog, download, verify, update, remove;
- **export** articles as Markdown with attribution front matter.

No local model needed: it works with whatever AI tool you already use.

## Install

Download the archive for your OS from GitHub Releases and unpack it anywhere:
`zim-mcp` and the libzim libraries it needs sit side by side, nothing else to install.

Or build it with Cargo, against a libzim installed on your system:

```sh
# macOS (Homebrew's ICU is keg-only, so point at libzim directly)
brew install libzim
LIBZIM_DIR=$(brew --prefix libzim) cargo install zim-mcp

# Debian / Ubuntu
sudo apt install libzim-dev
cargo install zim-mcp
```

## Get some archives

```sh
zim-mcp library search wikipedia --lang eng          # browse the Kiwix catalog
zim-mcp library add wikipedia_en_all_nopic --dir ~/zim
zim-mcp library list --dir ~/zim
zim-mcp library update --dir ~/zim                   # newest versions; old files go once the new ones check out
zim-mcp library remove wikipedia_en_all_nopic --dir ~/zim
zim-mcp library status --dir ~/zim                   # downloads in progress
```

Downloads resume if interrupted, are checked against the sha256 Kiwix publishes,
and only appear in the folder (as `<name>_<YYYY-MM>.zim`) once they check out.
The folder *is* the library: copy ZIM files in by hand and they just work.

## Use it with your AI tool

The server offers `search`, `read` and `list_archives`, plus library tools
(`library_search`, `library_list`, `library_status`, and `library_add`,
`library_update`, `library_remove`, which ask you to approve each change) and
`export`, which saves articles as Markdown once you approve.
Pass `--no-library` to hide the library tools and `export`; small models do better with fewer.

**Claude Code**

```sh
claude mcp add wiki -- zim-mcp serve --dir ~/zim
```

**Claude Desktop / LM Studio / Jan** — add to the app's MCP config
(`claude_desktop_config.json`, LM Studio's `mcp.json`):

```json
{ "mcpServers": { "wiki": { "command": "/path/to/zim-mcp", "args": ["serve", "--dir", "/path/to/zim"] } } }
```

**Pi, or any agent that prefers CLI tools** — the same operations as commands,
printing Markdown (or JSON with `--json`):

```sh
zim-mcp search "photosynthesis" --dir ~/zim
zim-mcp read wikipedia_en_all_nopic/Photosynthesis --page 2 --dir ~/zim
zim-mcp read wikipedia_en_all_nopic/Photosynthesis --section "Evolution" --dir ~/zim
zim-mcp list --dir ~/zim
```

**llama.cpp / llamafile** — `--mcp-servers-json '{"mcpServers":{"wiki":{"command":"zim-mcp","args":["serve","--dir","/path/to/zim","--no-library"]}}}'`

### Approving library changes

Over MCP, adding, updating or removing an archive, or exporting articles, always asks *you* (through MCP
elicitation) with the archive, its size and anything that will be deleted; the
model can't approve for you. If the app can't ask, the tool refuses and returns
the equivalent CLI command. If your app already prompts you before every tool
call, `--trust-client-approval` relies on that instead. Approved downloads run
in the background; `library_status` shows progress.

## Export as Markdown

```sh
zim-mcp export ~/zim/wikipedia_en_all_nopic_2026-06.zim Photosynthesis Chlorophyll --out notes/
zim-mcp export ~/zim/wikipedia_en_all_nopic_2026-06.zim --all --out notes/   # resumable
```

Files land in `<out>/<archive>/<first two characters>/<path>.md`, start with
YAML front matter (title, archive, path, date, source, license), and link to each
other with relative links, so the folder can be browsed as is. Redirects become
short files pointing at their target. The archive is never modified.

## Whole Earth drive

`bundle/` builds an optional drive you plug into any Mac, Windows or Linux
computer: Wikipedia, `zim-mcp`, a small local model and llamafile, with a
double-click launcher that opens a chat page in your browser. No network, no install.

```sh
just pin-release v0.1.0         # pin a zim-mcp release in bundle/manifest.sha256
just assemble /Volumes/WHOLE-EARTH
```

## License

GPL-3.0-or-later. Release archives include libzim (GPL-2.0-or-later) and a
`SOURCE.txt` pointing to the exact source of both.
