#!/usr/bin/env bash
# Build README.md, AGENT.md, and CONTRIBUTING.md from theme files
#
# Usage:
#   ./build-docs.sh
#
# Requires: pandoc

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
SOURCES="$ROOT_DIR/pandoc/sources"
PANDOC_SCRIPTS="$ROOT_DIR/pandoc/scripts"
README="$ROOT_DIR/README.md"
AGENT="$ROOT_DIR/AGENT.md"
CONTRIBUTING="$ROOT_DIR/CONTRIBUTING.md"

if ! command -v pandoc &>/dev/null; then
    echo "Error: pandoc is required but not found" >&2
    echo "Install pandoc or use nix develop" >&2
    exit 1
fi

# Check theme files for rule violations
for theme in "$SOURCES"/themes/*.mkd; do
    CHECK_OUTPUT=$(pandoc "$theme" \
        --from markdown+hard_line_breaks \
        --lua-filter="$PANDOC_SCRIPTS/check-rules.lua" \
        -o /dev/null 2>&1 | grep -v "^\[WARNING\]" | grep -v "^  Defaulting") || true

    if [[ -n "$CHECK_OUTPUT" ]]; then
        echo "$CHECK_OUTPUT" >&2
        exit 1
    fi
done

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
        --lua-filter="$PANDOC_SCRIPTS/remove-hard-breaks.lua" \
        --lua-filter="$PANDOC_SCRIPTS/filter-audience.lua" \
        --lua-filter="$PANDOC_SCRIPTS/promote-headings.lua" \
        --lua-filter="$PANDOC_SCRIPTS/fix-callouts.lua" \
        --metadata=audience:"$audience" \
        --metadata=include_dir:"$ROOT_DIR" \
        --wrap=preserve \
        -o "$output"
}

build_doc "$SOURCES/readme.mkd"       "$README"       readme
build_doc "$SOURCES/agent.mkd"        "$AGENT"        agent
build_doc "$SOURCES/contributing.mkd" "$CONTRIBUTING" contributing

echo "Generated:"
echo "  $README"
echo "  $AGENT"
echo "  $CONTRIBUTING"
