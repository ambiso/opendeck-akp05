if (( $# != 2 )); then
    echo "Usage: package-plugin BINARIES_DIRECTORY OUTPUT_ZIP" >&2
    exit 1
fi

binaries=$(realpath -- "$1")
archive=$(realpath -m -- "$2")
plugin_id=$(jq -er '.PluginUUID + ".sdPlugin"' "$manifest_path")
# zip updates existing archives; reject an old output to exclude stale files.
if [[ -e "$archive" ]]; then
    echo "Output already exists: $archive (choose a new path)" >&2
    exit 1
fi

# A release must contain every supported platform; don't silently ship partial ZIPs.
for binary in opendeck-akp05-linux opendeck-akp05-mac opendeck-akp05-win.exe; do
    test -s "$binaries/$binary" || { echo "Missing binary: $binary" >&2; exit 1; }
done
file "$binaries/opendeck-akp05-linux" | grep -q 'ELF.*x86-64'
file "$binaries/opendeck-akp05-mac" | grep -q 'Mach-O universal binary'
file "$binaries/opendeck-akp05-win.exe" | grep -q 'PE32+.*x86-64'

stage=$(mktemp -d)
trap 'rm -r -- "$stage"' EXIT
mkdir -p "$stage/$plugin_id" "$(dirname "$archive")"
install -m644 "$manifest_path" "$stage/$plugin_id/manifest.json"
cp -r --no-preserve=mode "$assets_path" "$stage/$plugin_id/assets"
for binary in opendeck-akp05-linux opendeck-akp05-mac opendeck-akp05-win.exe; do
    install -m755 "$binaries/$binary" "$stage/$plugin_id/$binary"
done
cd "$stage"
zip -qr "$archive" "$plugin_id"
