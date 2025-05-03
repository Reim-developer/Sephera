#!/bin/bash

 THREADS=$(sysctl -n hw.logicalcpu)
brew install ccache
              
python3 -m nuitka main.py \
    --onefile \
    --clang \
    --show-progress \
    --static-libpython=yes \
    --lto=yes \
    --jobs="$THREADS" \
    --output-dir=build/cli \
    --output-filename=sephera