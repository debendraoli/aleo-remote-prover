#!/bin/bash
# Install script for Aleo Remote Prover systemd service
# Run as root or with sudo

set -e

PROVER_USER="aleo-prover"
PROVER_HOME="/var/lib/aleo-prover"
ALEO_DIR="${PROVER_HOME}/.aleo"

echo "=== Aleo Remote Prover Service Setup ==="

# Create service user if it doesn't exist
if ! id "$PROVER_USER" &>/dev/null; then
    echo "Creating user: $PROVER_USER"
    useradd --system --no-create-home --shell /usr/sbin/nologin "$PROVER_USER"
fi

# Create directories
echo "Creating directories..."
mkdir -p "$PROVER_HOME"
mkdir -p "$ALEO_DIR"
mkdir -p /opt/aleo-prover

# Set ownership
chown -R "$PROVER_USER:$PROVER_USER" "$PROVER_HOME"
chown -R "$PROVER_USER:$PROVER_USER" /opt/aleo-prover

# Install systemd service
echo "Installing systemd service..."
cp "$(dirname "$0")/aleo-prover.service" /etc/systemd/system/

# Reload systemd
systemctl daemon-reload

echo ""
echo "=== Setup Complete ==="
echo ""
echo "Next steps:"
echo "  1. Install the prover binary to /usr/bin/aleo-remote-prover"
echo "     (e.g., from the .deb package or manual build)"
echo ""
echo "  2. Configure the service by editing /etc/systemd/system/aleo-prover.service"
echo "     Key settings:"
echo "       - PROVER_LISTEN_ADDR: Address to listen on (default: 127.0.0.1:3030)"
echo "       - MAX_CONCURRENT_PROOFS: Number of parallel proofs (default: 1)"
echo "       - ALEO_HOME: Directory for snarkvm data (default: ${ALEO_DIR})"
echo ""
echo "  3. Enable and start the service:"
echo "       sudo systemctl enable aleo-prover"
echo "       sudo systemctl start aleo-prover"
echo ""
echo "  4. Check status and logs:"
echo "       sudo systemctl status aleo-prover"
echo "       sudo journalctl -u aleo-prover -f"
echo ""
echo "Note: First startup will download ~1.5GB of snarkvm parameters."
echo "      This may take several minutes. Check logs for progress."
