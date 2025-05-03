#!/bin/bash

THREADS=$(nproc)
python3 -m nuitka main.py \
        --onefile \
        --clang \
        --lto=yes \
        --jobs="$THREADS" \
        --show-progress \
        --static-libpython=yes \
        --output-dir=build/cli \
        --output-filename=sephera