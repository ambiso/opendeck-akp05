if (( $# != 3 )); then
    echo "Usage: package-plugin BINARIES_DIRECTORY WINDOWS_ZIP OUTPUT_ZIP" >&2
    exit 1
fi

binaries=$(realpath -- "$1")
windows_zip=$(realpath -- "$2")
archive=$(realpath -m -- "$3")
plugin_id=$(jq -er '.PluginUUID + ".sdPlugin"' "$manifest_path")
if [[ -e "$archive" ]]; then
    echo "Output already exists: $archive" >&2
    exit 1
fi

stage=$(mktemp -d)
trap 'rm -r -- "$stage"' EXIT
unzip -q "$windows_zip" -d "$stage"
dir="$stage/$plugin_id"
jq -e --slurpfile expected "$manifest_path" '. == $expected[0]' "$dir/manifest.json" >/dev/null
win=$(jq -er .CodePathWin "$manifest_path")
file "$dir/$win" | grep -q 'PE32+.*x86-64'
for asset in transport.dll vcruntime140.dll vcruntime140_1.dll msvcp140.dll LICENSE LICENSE-Mirabox.txt jpeg-notices/LICENSE.md; do
    test -s "$dir/$asset" || { echo "Missing Windows package file: $asset" >&2; exit 1; }
done

linux=$(jq -er .CodePathLin "$manifest_path")
mac=$(jq -er .CodePathMac "$manifest_path")
file "$binaries/$linux" | grep -q 'ELF.*x86-64'
file "$binaries/$mac" | grep -q 'Mach-O universal binary'
install -m755 "$binaries/$linux" "$binaries/$mac" "$dir/"
mkdir -p "$(dirname "$archive")"
cd "$stage"
zip -qr "$archive" "$plugin_id"
