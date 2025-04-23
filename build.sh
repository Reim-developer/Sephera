#!/bin/bash
set -e

ENTRY="main.py"
OUTPUT="build"
OS=$(uname -s)
THREADS=$(nproc)

if [[ "$OS" == "Linux" ]];then
  if command -v python3 &> /dev/null; then
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
      $ENTRY

  else 
      echo "Python is not installed in your Linux system. Stop now."
  fi

elif [[ "$OS" == "Darwin" ]]; then
  if command -v python3 &> /dev/null; then
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
      $ENTRY

  else 
      echo "Python is not installed in your Darwin system. Stop now."
  fi

else
  echo "Unsupported operating system: $OS. If you're use Windows, please install via:"
  echo "https://github.com/reim-developer/Sephera/releases"
fi
