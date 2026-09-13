#!/bin/sh
# Built by CI; run manually to check the downstream high-level surface.
set -eu
cd "$(dirname "$0")"
cargo build --offline 2>/dev/null || cargo build
