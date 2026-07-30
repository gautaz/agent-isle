#!/usr/bin/env bash
# Build README.md, AGENTS.md, and CONTRIBUTING.md from theme files
#
# Usage:
#   ./build-docs.sh
#
# Requires: pandoc, panache

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
SOURCES="$ROOT_DIR/pandoc/sources"
PANDOC_SCRIPTS="$ROOT_DIR/pandoc/scripts"
README="$ROOT_DIR/README.md"
AGENT="$ROOT_DIR/AGENTS.md"
CONTRIBUTING="$ROOT_DIR/CONTRIBUTING.md"

if ! command -v pandoc &>/dev/null; then
    echo "Error: pandoc is required but not found" >&2
    echo "Install pandoc or use nix develop" >&2
    exit 1
fi

if ! command -v panache &>/dev/null; then
    echo "Error: panache is required but not found" >&2
    echo "Install panache or use nix develop" >&2
    exit 1
fi

# Generate output document from manifest
# Usage: build_doc <manifest> <output> <audience>
build_doc() {
    local manifest="$1"
    local output="$2"
    local audience="$3"

    pandoc "$manifest" \
        --from markdown+hard_line_breaks \
        --to gfm \
        --columns=120 \
        --lua-filter="$PANDOC_SCRIPTS/include.lua" \
        --lua-filter="$PANDOC_SCRIPTS/filter-audience.lua" \
        --lua-filter="$PANDOC_SCRIPTS/promote-headings.lua" \
        --lua-filter="$PANDOC_SCRIPTS/fix-callouts.lua" \
        --lua-filter="$PANDOC_SCRIPTS/strip-figures.lua" \
        --metadata=audience:"$audience" \
        --metadata=include_dir:"$ROOT_DIR" \
        --wrap=preserve \
        -o "$output"

    panache format "$output"
}

build_doc "$SOURCES/readme.mkd"       "$README"       readme
build_doc "$SOURCES/agents.mkd"       "$AGENT"        agent
build_doc "$SOURCES/contributing.mkd" "$CONTRIBUTING" contributing

echo "Generated:"
echo "  $README"
echo "  $AGENT"
echo "  $CONTRIBUTING"
