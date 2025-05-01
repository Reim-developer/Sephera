#!/bin/bash
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CFG_PATH="$PROJECT_ROOT/sephera-cpp/config.yml"
BUILD_DIR="$PROJECT_ROOT/sephera-cpp/build"

CC=clang
CXX=clang++

CXXFLAGS="-O3 -Wall -Wextra -g -march=native -mtune=native -funroll-loops -ftree-vectorize -flto=full -fomit-frame-pointer -DNDEBUG"
LDFLAGS="-Wl,--gc-sections -flto=full"

install_depdendencies() {
    echo "Install depdencencies..."

  
    sudo apt-get update

    sudo apt-get install -y \
        clang \
        qt6-base-dev \
        qt6-base-dev-tools \
        ninja-build \
        cmake \
        libyaml-cpp-dev \
        libssl-dev \
        zlib1g-dev
}

build_sephera_cpp() {
    mkdir -p "$BUILD_DIR"

    cp "$CFG_PATH" "$BUILD_DIR/config.yml"
    cd "$BUILD_DIR" || exit 1

     cmake -G "Ninja" .. \
        -DCMAKE_C_COMPILER="$CC" \
        -DCMAKE_CXX_COMPILER="$CXX" \
        -DCMAKE_CXX_FLAGS="$CXXFLAGS" \
        -DCMAKE_EXE_LINKER_FLAGS="$LDFLAGS" \
        -DCMAKE_BUILD_TYPE=Release \
        -DFORCE_OPTIMIZED=1

    ninja
}

main(){
    install_depdendencies
    build_sephera_cpp
}
main "$@"