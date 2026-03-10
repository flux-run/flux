#!/usr/bin/env bash
# Fluxbase CLI installer
# Usage: curl -fsSL https://fluxbase.co/install | bash
#
# Installs the `flux` binary to /usr/local/bin (or ~/.local/bin on Linux
# when /usr/local/bin is not writable without sudo).

set -euo pipefail

REPO="shashisrun/fluxbase"   # GitHub org/repo — update once repo is public
BASE_URL="https://github.com/$REPO/releases/latest/download"

# ─── Styling ────────────────────────────────────────────────────────────────
BOLD="$(tput bold 2>/dev/null || true)"
RESET="$(tput sgr0 2>/dev/null || true)"
GREEN="$(tput setaf 2 2>/dev/null || true)"
YELLOW="$(tput setaf 3 2>/dev/null || true)"
RED="$(tput setaf 1 2>/dev/null || true)"

info()    { echo "${BOLD}${GREEN}▸${RESET} $*"; }
warning() { echo "${BOLD}${YELLOW}⚠${RESET}  $*"; }
error()   { echo "${BOLD}${RED}✗${RESET}  $*" >&2; exit 1; }
success() { echo "${BOLD}${GREEN}✔${RESET} $*"; }

# ─── Detect OS + ARCH ───────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)                   BINARY="flux-darwin-arm64" ;;
      x86_64)                  BINARY="flux-darwin-amd64" ;;
      *)                       error "Unsupported macOS architecture: $ARCH" ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      aarch64|arm64)           BINARY="flux-linux-arm64" ;;
      x86_64|amd64)            BINARY="flux-linux-amd64" ;;
      *)                       error "Unsupported Linux architecture: $ARCH" ;;
    esac
    ;;
  MINGW*|CYGWIN*|MSYS*)
    case "$ARCH" in
      aarch64|arm64)           BINARY="flux-windows-arm64.exe" ;;
      x86_64|amd64)            BINARY="flux-windows-amd64.exe" ;;
      *)                       error "Unsupported Windows architecture: $ARCH" ;;
    esac
    ;;
  *)
    error "Unsupported OS: $OS. Download manually from https://fluxbase.co/docs/install"
    ;;
esac

# ─── Choose install dir ──────────────────────────────────────────────────────
INSTALL_DIR="/usr/local/bin"
if [[ ! -w "$INSTALL_DIR" ]]; then
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
  warning "/usr/local/bin is not writable — installing to $INSTALL_DIR"
  # Remind the user to add to PATH if needed
  if ! echo "$PATH" | grep -q "$HOME/.local/bin"; then
    warning "Add $HOME/.local/bin to your PATH:"
    warning "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc   # bash"
    warning "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.zshrc    # zsh"
    echo ""
  fi
fi

INSTALL_PATH="$INSTALL_DIR/flux"

# ─── Download ────────────────────────────────────────────────────────────────
DOWNLOAD_URL="$BASE_URL/$BINARY"
TMP="$(mktemp)"

info "Detecting platform: $OS / $ARCH → $BINARY"
info "Downloading from $DOWNLOAD_URL ..."

if command -v curl &>/dev/null; then
  curl -fsSL --progress-bar "$DOWNLOAD_URL" -o "$TMP"
elif command -v wget &>/dev/null; then
  wget -q --show-progress "$DOWNLOAD_URL" -O "$TMP"
else
  error "curl or wget is required to install Fluxbase CLI."
fi

chmod +x "$TMP"

# ─── Verify binary ───────────────────────────────────────────────────────────
if ! "$TMP" --version &>/dev/null; then
  error "Downloaded binary failed to execute. Please report this at https://github.com/$REPO/issues"
fi

VERSION=$("$TMP" --version 2>&1 | awk '{print $NF}')

# ─── Install ─────────────────────────────────────────────────────────────────
mv "$TMP" "$INSTALL_PATH"

echo ""
success "Fluxbase CLI $VERSION installed to $INSTALL_PATH"
echo ""
echo "  Run:"
echo ""
echo "    flux login"
echo "    flux create my-app"
echo "    cd my-app && flux deploy"
echo ""
echo "  Docs: https://fluxbase.co/docs/quickstart"
echo ""
