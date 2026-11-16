# zim-mcp and zimfile: build and test (pure Rust; `just testdata` first).
# Whole Earth AI: `just fetch`, then `just assemble /Volumes/WHOLE-EARTH`.

repo := env("ZIM_MCP_REPO", "gordonbrander/zim-mcp")
dl := "bundle/downloads"
manifest := "bundle/manifest.sha256"

build:
    cargo build --release

test:
    cargo test --workspace

# Test data (gitignored): openZIM's zim-testing-suite at a pinned commit, and
# a small real archive for zim-mcp's end-to-end golden tests.
testdata:
    #!/usr/bin/env bash
    set -euo pipefail
    suite=2edf72096208e60c82b6ebbe313baa552cc6af52
    d=crates/zimfile/tests/data
    if [ ! -f "$d/.commit" ] || [ "$(cat "$d/.commit")" != "$suite" ]; then
      rm -rf "$d" && mkdir -p "$d"
      curl -fsSL "https://github.com/openzim/zim-testing-suite/archive/$suite.tar.gz" \
        | tar xz -C "$d" --strip-components=2 "zim-testing-suite-$suite/data"
      echo "$suite" > "$d/.commit"
    fi
    f=crates/zim-mcp/tests/data/wikipedia_en_nollywood_maxi_2026-10.zim
    if [ ! -f "$f" ]; then
      mkdir -p "$(dirname "$f")"
      curl -fL --retry 3 -o "$f.part" "https://download.kiwix.org/zim/wikipedia/$(basename "$f")"
      mv "$f.part" "$f"
    fi
    echo "faa53e0c8ef641206867ce21f0e107d4e2aa9012e3b2263f05fe9bddf4f72aa3  $f" | (if command -v shasum >/dev/null; then shasum -a 256 -c; else sha256sum -c; fi)

# Download llamafile, the model and the zim-mcp release archives in the manifest, and check them.
# Files already in bundle/downloads are only checked. zim-mcp releases come through `gh`.
fetch:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p {{dl}}
    grep -E '^[0-9a-f]{64} ' {{manifest}} | while read -r sha file url; do
      [ -f "{{dl}}/$file" ] && continue
      case "$url" in
        # This repo's own releases: gh also works while the repo is private.
        https://github.com/{{repo}}/releases/download/*)
          tag=${url#https://github.com/{{repo}}/releases/download/}; tag=${tag%%/*}
          gh release download "$tag" --repo {{repo}} --pattern "$file" --dir {{dl}} ;;
        *)
          curl -fL --retry 3 -C - -o "{{dl}}/$file.part" "$url"
          mv "{{dl}}/$file.part" "{{dl}}/$file" ;;
      esac
    done
    grep -E '^[0-9a-f]{64} ' {{manifest}} | awk '{print $1"  "$2}' | (cd {{dl}} && shasum -a 256 -c)

# Lay out a drive at DEST (an exFAT drive) from the manifest.
assemble dest: fetch
    #!/usr/bin/env bash
    set -euo pipefail
    d="{{dest}}"; dl="{{dl}}"
    mkdir -p "$d/bin" "$d/models" "$d/zim" "$d/LICENSES"
    cp "$dl/llamafile.exe" "$d/llamafile.exe" && chmod +x "$d/llamafile.exe"
    cp "$dl/model.gguf" "$d/models/model.gguf"
    # zim-mcp-<version>-<os>.tar.gz|zip → bin/<os>/, unmodified.
    for a in "$dl"/zim-mcp-*; do
      base=$(basename "$a"); stem=${base%.tar.gz}; stem=${stem%.zip}
      os=$(echo "$stem" | sed -E 's/^zim-mcp-[^-]+-//')
      tmp=$(mktemp -d)
      case "$a" in *.zip) unzip -q "$a" -d "$tmp" ;; *) tar xzf "$a" -C "$tmp" ;; esac
      rm -rf "$d/bin/$os"; mv "$tmp/$stem" "$d/bin/$os"; rm -rf "$tmp"
    done
    [ -d "$d/bin/macos-universal" ] || { echo "no zim-mcp release in the manifest: run just pin-release <tag>"; exit 1; }
    cp bundle/launchers/* "$d/"
    chmod +x "$d/Start (Mac).command" "$d/start-linux.sh"
    cp bundle/licenses/* "$d/LICENSES/"
    cp LICENSE "$d/LICENSES/GPL-3.0.txt"
    cp "$d/bin/macos-universal/SOURCE.txt" "$d/LICENSES/zim-mcp-SOURCE.txt"
    case "$(uname -s)-$(uname -m)" in
      Darwin-*) zm="$d/bin/macos-universal/zim-mcp" ;;
      Linux-x86_64) zm="$d/bin/linux-x64/zim-mcp" ;;
      Linux-aarch64) zm="$d/bin/linux-arm64/zim-mcp" ;;
      *) echo "assemble on macOS or Linux"; exit 1 ;;
    esac
    awk '$1 == "zim" {print $2}' {{manifest}} | while read -r name; do
      "$zm" library add "$name" --dir "$d/zim"
    done
    echo "Drive ready at $d"

# Pin a zim-mcp GitHub release (e.g. v0.1.0) in the manifest.
pin-release tag:
    #!/usr/bin/env bash
    set -euo pipefail
    base="https://github.com/{{repo}}/releases/download/{{tag}}"
    sums=$(gh release download {{tag}} --repo {{repo}} --pattern SHA256SUMS --output -)
    grep -v ' zim-mcp-' {{manifest}} > {{manifest}}.tmp
    echo "$sums" | awk -v b="$base" '{print $1"  "$2"  "b"/"$2}' > {{manifest}}.new
    # Keep the release lines right after their comment.
    awk -v f={{manifest}}.new '{print} /just pin-release/ {while ((getline l < f) > 0) print l}' {{manifest}}.tmp > {{manifest}}
    rm {{manifest}}.tmp {{manifest}}.new
    grep ' zim-mcp-' {{manifest}}
