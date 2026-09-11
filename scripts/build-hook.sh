#!/bin/sh
# The placeholder exists because tauri-build refuses to run until the sidecar
# is present, and building the hook runs tauri-build.
set -e

target=$(rustc -vV | sed -n 's/host: //p')
staged="src-tauri/binaries/ledge-hook-$target"

mkdir -p src-tauri/binaries
[ -f "$staged" ] || touch "$staged"

if [ "${1:-release}" = "dev" ]; then
  cargo build --bin ledge-hook --manifest-path src-tauri/Cargo.toml
  profile=debug
else
  cargo build --release --bin ledge-hook --manifest-path src-tauri/Cargo.toml
  profile=release
fi

cp "src-tauri/target/$profile/ledge-hook" "$staged"
chmod +x "$staged"
