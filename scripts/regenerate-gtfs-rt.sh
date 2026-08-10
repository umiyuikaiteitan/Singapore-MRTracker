#!/usr/bin/env bash
# Regenerate the vendored GTFS-Realtime protobuf bindings.
#
# Run this script only when Google publishes a new version of
# gtfs-realtime.proto. The script needs `protoc` and the `prost-build`
# crate. The generated file is committed to the repository, so users of
# the library do not need `protoc`.
#
# Usage:
#   ./scripts/regenerate-gtfs-rt.sh
set -euo pipefail

cd "$(dirname "$0")/.."

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

cp crates/mrt-gtfs-rt/proto/gtfs-realtime.proto "$WORKDIR/"

cd "$WORKDIR"
cargo init -q --name protogen
cargo add -q prost-build@0.14
cat > src/main.rs <<'EOF'
fn main() {
    let mut config = prost_build::Config::new();
    config.out_dir(".");
    config
        .compile_protos(&["gtfs-realtime.proto"], &["."])
        .unwrap();
}
EOF
cargo run -q

cd - > /dev/null
cp "$WORKDIR/transit_realtime.rs" crates/mrt-gtfs-rt/src/transit_realtime.rs
echo "Updated crates/mrt-gtfs-rt/src/transit_realtime.rs"
