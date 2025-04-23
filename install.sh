#!/bin/bash
set -e

REPO="Reim-developer/Sephera"
BINARY_NAME="sephera"
INSTALL_PATH="/usr/local/bin/$BINARY_NAME"

OS=$(uname -s)

case "$OS" in
  Linux)
    FILE_NAME="sephera_linux"
    ;;
  Darwin)
    FILE_NAME="sephera_macos"
    ;;
  *)
    echo "❌ Unsupported OS: $OS"
    echo "👉 On Windows, please download manually:"
    echo "   https://github.com/$REPO/releases"
    exit 1
    ;;
esac

URL="https://github.com/$REPO/releases/latest/download/$FILE_NAME"

echo "🔽 Downloading $FILE_NAME from $URL"
curl -L "$URL" -o "$BINARY_NAME"

chmod +x "$BINARY_NAME"

echo "🚚 Installing to $INSTALL_PATH"
sudo mv "$BINARY_NAME" "$INSTALL_PATH"

echo "✅ Done! Try running: $BINARY_NAME --help"
