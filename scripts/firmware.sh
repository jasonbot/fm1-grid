#!/bin/sh
# Manage the embedded firmware list (firmware/firmwares.json); requires jq.
#
#   scripts/firmware.sh add <file.fwsc> <name> <version> [tags] [url]
#   scripts/firmware.sh del <name-or-file>
#   scripts/firmware.sh list
#
# `tags` is comma-separated; the first tag is the dropdown group. `url` is the download
# location recorded for firmware/download (omit it for local-only packages). After
# add/del, rebuild with `cargo build --release`. The default slot contents follow the
# JSON order, so the first entries are what starts with no CLI arguments.
set -eu

cd "$(dirname "$0")/.."
JSON=firmware/firmwares.json

usage() {
    echo "usage:" >&2
    echo "  $0 add <file.fwsc> <name> <version> [tags] [url]" >&2
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

# Reads the whole JSON array on stdin and rewrites $JSON in the file's style:
# one entry per line.
save() {
    tmp=$JSON.tmp
    cat > "$tmp"
    { echo '['; jq -c '.[]' "$tmp" | sed '$!s/$/,/'; echo ']'; } > "$JSON"
    rm -f "$tmp"
}

case "${1:-}" in
    add)
        { [ $# -ge 4 ] && [ $# -le 6 ]; } || usage
        src=$2
        name=$3
        version=$4
        tags=${5:-other}
        url=${6:-}
        [ -f "$src" ] || { echo "no such file: $src" >&2; exit 1; }
        file=$(basename "$src")
        dest=firmware/$file
        [ "$src" = "$dest" ] || cp "$src" "$dest"
        sum=$(sha256_of "$dest")
        tags_json=$(printf '%s' "$tags" |
            jq -R 'split(",") | map(gsub("^\\s+|\\s+$"; "")) | map(select(length > 0))')
        [ "$tags_json" != "[]" ] || { echo "tags must not be empty" >&2; exit 1; }
        # Replace any existing entry with the same file or name, keeping its recorded
        # url (unless overridden), source and license.
        jq --arg file "$file" --arg name "$name" --arg version "$version" \
           --arg sha256 "$sum" --arg url "$url" --argjson tags "$tags_json" '
            (map(select(.file == $file or .name == $name)) | .[0] // {}) as $old
            | map(select(.file != $file and .name != $name))
            + [ {
                  name: $name,
                  version: $version,
                  file: $file,
                  tags: $tags,
                  url: (if $url != "" then $url else $old.url end),
                  source: $old.source,
                  sha256: $sha256,
                  license: $old.license
                } | with_entries(select(.value != null)) ]
        ' "$JSON" | save
        echo "added: $name $version ($file, sha256 $sum, tags $tags)"
        echo "rebuild with: cargo build --release"
        ;;
    del)
        [ $# -eq 2 ] || usage
        what=$2
        # Match by name, or treat the argument as a file name.
        file=$(jq -r --arg what "$what" \
            '.[] | select(.name == $what or .file == $what) | .file' "$JSON" | head -1)
        [ -n "$file" ] || file=$what
        jq --arg file "$file" --arg what "$what" \
            'map(select(.file != $file and .name != $what))' "$JSON" | save
        rm -f "firmware/$file"
        echo "removed: $what"
        echo "rebuild with: cargo build --release"
        ;;
    list)
        rows=$(jq -r '.[] | [.file, .name, .version, .sha256, (.tags | join(","))] | join(" | ")' "$JSON")
        printf '%s\n' "$rows" | column -t -s'|' 2>/dev/null || printf '%s\n' "$rows"
        ;;
    *)
        usage
        ;;
esac
