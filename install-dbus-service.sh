#!/bin/bash
# Install D-Bus service file for runst notification daemon

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNST_BIN="${SCRIPT_DIR}/target/release/runst"

# Check if runst binary exists
if [[ ! -f "$RUNST_BIN" ]]; then
    echo "Error: runst binary not found at $RUNST_BIN"
    echo "Please build first with: cargo build --release"
    exit 1
fi

# Create D-Bus services directory
DBUS_DIR="$HOME/.local/share/dbus-1/services"
mkdir -p "$DBUS_DIR"

# Write the service file
SERVICE_FILE="$DBUS_DIR/org.freedesktop.Notifications.service"
cat > "$SERVICE_FILE" << EOF
[D-BUS Service]
Name=org.freedesktop.Notifications
Exec=$RUNST_BIN
EOF

echo "Installed D-Bus service file: $SERVICE_FILE"
echo ""
cat "$SERVICE_FILE"
echo ""
echo "runst will now auto-start when a notification is sent."
echo ""
echo "To test: notify-send 'Test' 'Hello from runst'"
