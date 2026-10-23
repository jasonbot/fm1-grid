#!/bin/sh
# Manage the embedded firmware list (firmware/manifest.txt).
#
#   scripts/firmware.sh add <file.fwsc> <name> <version> [tags]
#   scripts/firmware.sh del <name-or-file>
#   scripts/firmware.sh list
#
# After add/del, rebuild with `cargo build --release`. The default slot contents follow the
# manifest order, so the first lines are what starts with no CLI arguments.
set -eu

cd "$(dirname "$0")/.."
MANIFEST=firmware/manifest.txt

usage() {
    echo "usage:" >&2
    echo "  $0 add <file.fwsc> <name> <version> [tags]" >&2
    echo "  $0 del <name-or-file>" >&2
    echo "  $0 list" >&2
    exit 2
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

case "${1:-}" in
    add)
        { [ $# -eq 4 ] || [ $# -eq 5 ]; } || usage
        src=$2
        name=$3
        version=$4
        tags=${5:-other}
        [ -f "$src" ] || { echo "no such file: $src" >&2; exit 1; }
        file=$(basename "$src")
        dest=firmware/$file
        [ "$src" = "$dest" ] || cp "$src" "$dest"
        sum=$(sha256_of "$dest")
        # Replace any existing entry with the same file or name.
        grep -Fv -e "$file | " -e " | $name | " "$MANIFEST" > "$MANIFEST.tmp" || true
        mv "$MANIFEST.tmp" "$MANIFEST"
        echo "$file | $name | $version | $sum | $tags" >> "$MANIFEST"
        echo "added: $name $version ($file, sha256 $sum, tags $tags)"
        echo "rebuild with: cargo build --release"
        ;;
    del)
        [ $# -eq 2 ] || usage
        what=$2
        # Match by name, or treat the argument as a file name.
        if grep -qF " | $what | " "$MANIFEST"; then
            file=$(grep -F " | $what | " "$MANIFEST" | head -1 | cut -d'|' -f1 | tr -d ' ')
        else
            file=$what
        fi
        grep -Fv -e "$file | " -e " | $what | " "$MANIFEST" > "$MANIFEST.tmp" || true
        mv "$MANIFEST.tmp" "$MANIFEST"
        rm -f "firmware/$file"
        echo "removed: $what"
        echo "rebuild with: cargo build --release"
        ;;
    list)
        column -t -s'|' "$MANIFEST" 2>/dev/null || cat "$MANIFEST"
        ;;
    *)
        usage
        ;;
esac
