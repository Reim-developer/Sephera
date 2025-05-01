#!/bin/bash
os_type=$(uname -s)
program_name="sephera-cpp"
cfg_path="config.yml"
build_dir="build"
exit_code=$?

clang_detect() {
    if command -v clang >/dev/null 2>&1; then
        echo "[OK] Clang is already install in your os..."
    else
        echo "[BAD] Clang not found in your os. Please install them before build."
        exit 1
    fi
}

cmake_detect() {
    if command -v cmake >/dev/null 2>&1; then
        echo "[OK] Cmake is already install in your os..."
    else
        echo "[BAD] CMake not found in your os. Please install them before build."
        exit 1
    fi
}

os_detect() {
    case $os_type in "Linux")
        clang_detect
        cmake_detect
        
        cp "$cfg_path" "$build_dir"
        cd build || exit 1
        cmake .. 
        make 
        ./"$program_name"

        echo "Build SUCCESS | With exit status code: $exit_code"
    ;;
        
    esac
}

os_detect