#!/usr/bin/env bash
set -euo pipefail

# Build a simple .deb containing:
# - /usr/bin/remote-prover
# - /lib/systemd/system/remote-prover.service
# - /etc/default/remote-prover
# - maintainer scripts (postinst/prerm)
#
# Usage:
#   ./scripts/build_deb.sh <version> [target_triple] [arch] [out_dir] [network]
# Example:
#   ./scripts/build_deb.sh 0.1.0 x86_64-unknown-linux-gnu amd64 artifacts testnet

VERSION_RAW=${1:?"version required (e.g. 0.1.0 or v0.1.0)"}
TARGET_TRIPLE=${2:-x86_64-unknown-linux-gnu}
ARCH=${3:-amd64}
OUT_DIR=${4:-artifacts}
NETWORK=${5:-mainnet}

VERSION=${VERSION_RAW#v}

BIN_PATH="target/${TARGET_TRIPLE}/release/remote-prover"
if [[ ! -f "$BIN_PATH" ]]; then
  echo "Binary not found at $BIN_PATH (did you build --release --target $TARGET_TRIPLE?)" >&2
  exit 1
fi

if [[ ! -f packaging/remote-prover.service ]]; then
  echo "Missing packaging/remote-prover.service" >&2
  exit 1
fi

if [[ ! -f packaging/remote-prover.default ]]; then
  echo "Missing packaging/remote-prover.default" >&2
  exit 1
fi

STAGE_DIR=$(mktemp -d)
trap 'rm -rf "$STAGE_DIR"' EXIT

PKG_DIR="$STAGE_DIR/remote-prover_${VERSION}_${ARCH}"
mkdir -p \
  "$PKG_DIR/DEBIAN" \
  "$PKG_DIR/usr/bin" \
  "$PKG_DIR/lib/systemd/system" \
  "$PKG_DIR/etc/default"

install -m 0755 "$BIN_PATH" "$PKG_DIR/usr/bin/remote-prover"
install -m 0644 packaging/remote-prover.service "$PKG_DIR/lib/systemd/system/remote-prover.service"
install -m 0644 packaging/remote-prover.default "$PKG_DIR/etc/default/remote-prover"

cat > "$PKG_DIR/DEBIAN/control" <<EOF
Package: remote-prover
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: debendraoli <noreply@github.com>
Depends: libc6 (>= 2.31), ca-certificates, adduser, systemd
Description: Remote Aleo Prover (${NETWORK})
 A lightweight HTTP service that executes Aleo authorizations using SnarkVM.
 Built for ${NETWORK} network.
EOF

install -m 0755 packaging/debian/postinst "$PKG_DIR/DEBIAN/postinst"
install -m 0755 packaging/debian/prerm "$PKG_DIR/DEBIAN/prerm"

mkdir -p "$OUT_DIR"
DEB_NAME="remote-prover-${NETWORK}_${VERSION}_${ARCH}.deb"

# Prefer fakeroot if available (GitHub-hosted runners usually have it)
if command -v fakeroot >/dev/null 2>&1; then
  fakeroot dpkg-deb --build "$PKG_DIR" "$OUT_DIR/$DEB_NAME" >/dev/null
else
  dpkg-deb --build "$PKG_DIR" "$OUT_DIR/$DEB_NAME" >/dev/null
fi

sha256sum "$OUT_DIR/$DEB_NAME" > "$OUT_DIR/$DEB_NAME.sha256"

echo "Built: $OUT_DIR/$DEB_NAME"
