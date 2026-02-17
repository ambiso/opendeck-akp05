id := "com.github.ambiso.opendeck-akp05.sdPlugin"

release: bump package tag

package: build-linux build-mac build-win collect zip

bump next=`git cliff --bumped-version | tr -d "v"`:
    git diff --cached --exit-code

    echo "We will bump version to {{next}}, press any key"
    read ans

    sed -i 's/"Version": ".*"/"Version": "{{next}}"/g' manifest.json
    sed -i 's/^version = ".*"$/version = "{{next}}"/g' Cargo.toml

tag next=`git cliff --bumped-version`:
    echo "Generating changelog"
    git cliff -o CHANGELOG.md --tag {{next}}

    echo "We will now commit the changes, please review before pressing any key"
    read ans

    git add .
    git commit -m "chore(release): {{next}}"
    git tag "{{next}}"

build-linux:
    cargo build --release --target x86_64-unknown-linux-gnu --target-dir target/plugin-linux
    cargo zigbuild --release --target aarch64-unknown-linux-gnu --target-dir target/plugin-linux

build-mac:
    -docker run --rm -v $(pwd):/io -w /io ghcr.io/rust-cross/cargo-zigbuild:sha-eba2d7e cargo zigbuild --release --target x86_64-apple-darwin --target-dir target/plugin-mac
    -docker run --rm -v $(pwd):/io -w /io ghcr.io/rust-cross/cargo-zigbuild:sha-eba2d7e cargo zigbuild --release --target aarch64-apple-darwin --target-dir target/plugin-mac

build-win:
    -cargo zigbuild --release --target x86_64-pc-windows-gnu --target-dir target/plugin-win
    -cargo zigbuild --release --target aarch64-pc-windows-gnullvm --target-dir target/plugin-win

clean:
    rm -rf target/

collect:
    rm -rf build
    mkdir -p build/{{id}}
    cp -r assets build/{{id}}
    cp manifest.json build/{{id}}
    -mkdir -p build/{{id}}/x86_64-unknown-linux-gnu/bin
    -cp target/plugin-linux/x86_64-unknown-linux-gnu/release/opendeck-akp05 build/{{id}}/x86_64-unknown-linux-gnu/bin/
    -mkdir -p build/{{id}}/aarch64-unknown-linux-gnu/bin
    -cp target/plugin-linux/aarch64-unknown-linux-gnu/release/opendeck-akp05 build/{{id}}/aarch64-unknown-linux-gnu/bin/
    -mkdir -p build/{{id}}/x86_64-apple-darwin/bin
    -cp target/plugin-mac/x86_64-apple-darwin/release/opendeck-akp05 build/{{id}}/x86_64-apple-darwin/bin/
    -mkdir -p build/{{id}}/aarch64-apple-darwin/bin
    -cp target/plugin-mac/aarch64-apple-darwin/release/opendeck-akp05 build/{{id}}/aarch64-apple-darwin/bin/
    -mkdir -p build/{{id}}/x86_64-pc-windows-gnu/bin
    -cp target/plugin-win/x86_64-pc-windows-gnu/release/opendeck-akp05.exe build/{{id}}/x86_64-pc-windows-gnu/bin/
    -mkdir -p build/{{id}}/aarch64-pc-windows-gnullvm/bin
    -cp target/plugin-win/aarch64-pc-windows-gnullvm/release/opendeck-akp05.exe build/{{id}}/aarch64-pc-windows-gnullvm/bin/

[working-directory: "build"]
zip:
    zip -r opendeck-akp05.plugin.zip {{id}}/
