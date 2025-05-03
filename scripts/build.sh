#!/bin/bash

cli_entry="main.py"
OUTPUT="build"
OS=$(uname -s)
THREADS=$(nproc)

detect_python() {
  if command -v python3 &> /dev/null; then
    echo "Python already installed in $OS"
  else
    echo "Python is not installed in $OS, exit now"
    exit 1
  fi
}

linux_build() {
  if [[ $"$OS" == "Linux" ]]; then
      detect_python

      # Build CLI version of Sephera.
      python3 -m nuitka \
      --onefile \
      --remove-output \
      --show-progress \
      --nofollow-import-to=tests,examples,test \
      --noinclude-pytest=nofollow \
      --lto=yes \
      --clang \
      --jobs="$THREADS" \
      --static-libpython=yes \
      --output-dir=$OUTPUT \
      $cli_entry
  fi
}

macos_build() {
  if [[ "$OS" == "Darwin" ]]; then
     detect_python

     # CLI Build
     python3 -m nuitka \
      --onefile \
      --remove-output \
      --show-progress \
      --nofollow-import-to=tests,examples,test \
      --noinclude-pytest=nofollow \
      --lto=yes \
      --clang \
      --jobs="$THREADS" \
      --static-libpython=yes \
      --output-dir=$OUTPUT \
      "$ENTRY"    
  fi
}

if [[ "$OS" == "Windows" ]]; then
  echo "Windows is not supported for build from Shell script"
  echo "Please read our documentation for install for Windows at:"
  echo "https://reim-developer.github.io/Sephera/pages/install"
  exit 1
fi

linux_build
macos_build
