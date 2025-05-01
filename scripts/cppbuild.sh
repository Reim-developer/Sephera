#!/bin/bash
os_type=$(uname -s)
program_name="sephera-cpp"
cfg_path="../sephera-cpp/config.yml"
build_dir="../sephera-cpp/build"
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
        
        if [ ! -d "$build_dir" ]; then
            echo "$build_dir doesn't exists. Create now.."
            mkdir -p "$build_dir"
        fi

        cp "$cfg_path" "$build_dir"
        cd "$build_dir" || exit 1
        
        cmake -G "Ninja" ..
        ninja
        ./"$program_name"

        echo "Build SUCCESS | With exit status code: $exit_code"
    ;;
        
    esac
}

os_detect