#!/bin/bash
set -e

# scripts/test-user-flow.sh
# Simulates a user installing flux via curl and running basic commands.

# 1. Build everything
echo "Building all binaries..."
cargo build --release --workspace

# 2. Setup a fake "install" directory
TEMP_INSTALL_DIR=$(mktemp -d)
echo "Installing to $TEMP_INSTALL_DIR..."
cp target/release/flux "$TEMP_INSTALL_DIR/"
cp target/release/flux-server "$TEMP_INSTALL_DIR/"
cp target/release/flux-runtime "$TEMP_INSTALL_DIR/"

# 3. Setup a fake "user" directory (no Cargo.toml here!)
USER_DIR=$(mktemp -d)
echo "Running as user in $USER_DIR..."
cd "$USER_DIR"

# 4. Add temp bin to PATH
export PATH="$TEMP_INSTALL_DIR:$PATH"

# 5. Test Init
echo "Testing flux init..."
mkdir my-app
cd my-app
flux init

# 6. Test Dev (This is where it currently fails)
echo "Testing flux dev..."
# We expect this to fail right now with "could not locate workspace root"
# We run it in the background and capture output
flux dev > flux_out.log 2>&1 &
FLUX_PID=$!
sleep 2
kill $FLUX_PID || true

if grep -q "could not locate workspace root" flux_out.log; then
    echo "FAILURE: flux dev failed as expected (reproduced the bug)."
    exit 1
else
    echo "SUCCESS: flux dev works (or failed with a different error)!"
    cat flux_out.log
fi

# Cleanup
rm -rf "$TEMP_INSTALL_DIR" "$USER_DIR"
