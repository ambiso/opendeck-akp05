id := "com.github.ambiso.opendeck-akp05.sdPlugin"

release: bump tag

package binaries="build/binaries":
    nix run .#package-plugin -- "{{binaries}}" build/opendeck-akp05.plugin.zip

install-local: build-linux
    mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/opendeck/plugins/{{id}}"
    install -m755 build/binaries/opendeck-akp05-linux "${XDG_CONFIG_HOME:-$HOME/.config}/opendeck/plugins/{{id}}/opendeck-akp05-linux"
    cp manifest.json "${XDG_CONFIG_HOME:-$HOME/.config}/opendeck/plugins/{{id}}/manifest.json"
    cp -r assets "${XDG_CONFIG_HOME:-$HOME/.config}/opendeck/plugins/{{id}}/"

bump next=`git cliff --bumped-version | tr -d "v"`:
    git diff --cached --exit-code

    echo "We will bump version to {{next}}, press any key"
    read ans

    sed -i 's/"Version": ".*"/"Version": "{{next}}"/g' manifest.json
    sed -i 's/^version = ".*"$/version = "{{next}}"/g' Cargo.toml
    nix develop --command cargo update --workspace

tag next=`git cliff --bumped-version`:
    echo "Generating changelog"
    git cliff -o CHANGELOG.md --tag {{next}}

    echo "We will now commit the changes, please review before pressing any key"
    read ans

    git add .
    git commit -m "chore(release): {{next}}"
    git tag "{{next}}"

build-linux:
    mkdir -p build/binaries
    nix build .#linux-static --out-link build/nix-linux
    install -m755 build/nix-linux/bin/opendeck-akp05 build/binaries/opendeck-akp05-linux

build-mac:
    mkdir -p build/binaries
    nix build .#macos-universal --out-link build/nix-macos
    install -m755 build/nix-macos/bin/opendeck-akp05 build/binaries/opendeck-akp05-mac

build-win:
    mkdir -p build/binaries
    nix build .#windows --out-link build/nix-windows
    install -m755 build/nix-windows/bin/opendeck-akp05.exe build/binaries/opendeck-akp05-win.exe

clean:
    sudo rm -rf target/
