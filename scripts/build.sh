#!/usr/bin/env bash
# Local development build script (Linux / macOS)
# Builds the plugin. Deploys to the test project only when --deploy is given.
#
# Usage:
#   ./scripts/build.sh                       # debug build only
#   ./scripts/build.sh --deploy              # debug build + deploy
#   ./scripts/build.sh --release             # release build only
#   ./scripts/build.sh --skip-checks         # skip cargo fmt/clippy
#   TEST_PROJECT=/path/to/project ./scripts/build.sh --deploy

set -euo pipefail

TEST_PROJECT="${TEST_PROJECT:-$HOME/dev/godot-camerafeed-demo}"
PROFILE=debug
DEPLOY=0
SKIP_CHECKS=0

for arg in "$@"; do
    case "$arg" in
        --deploy)      DEPLOY=1 ;;
        --release)     PROFILE=release ;;
        --skip-checks) SKIP_CHECKS=1 ;;
        *) echo "Unknown option: $arg" >&2; exit 1 ;;
    esac
done

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

if [ "$SKIP_CHECKS" -eq 0 ]; then
    echo ">> cargo fmt"
    cargo fmt
    echo ">> cargo clippy"
    cargo clippy
fi

echo ">> cargo build ($PROFILE)"
if [ "$PROFILE" = release ]; then
    cargo build --release
else
    cargo build
fi

# Map OS/arch to the bin directory used by godot-neovim.gdextension
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
    Linux)
        lib=libgodot_neovim.so
        case "$arch" in
            aarch64|arm64) bin_dir=linux-arm64 ;;
            x86_64)        bin_dir=linux ;;
            *) echo "Unsupported Linux arch: $arch" >&2; exit 1 ;;
        esac
        ;;
    Darwin)
        lib=libgodot_neovim.dylib
        case "$arch" in
            arm64)  bin_dir=macos-arm64 ;;
            x86_64) bin_dir=macos-x86_64 ;;
            *) echo "Unsupported macOS arch: $arch" >&2; exit 1 ;;
        esac
        ;;
    *)
        echo "Unsupported OS: $os (use scripts/build.ps1 on Windows)" >&2
        exit 1
        ;;
esac

artifact="$repo_root/target/$PROFILE/$lib"
if [ ! -f "$artifact" ]; then
    echo "Build artifact not found: $artifact" >&2
    exit 1
fi

if [ "$DEPLOY" -eq 0 ]; then
    echo "Done: $PROFILE build ($artifact)"
    echo "(use --deploy to copy to the test project)"
    exit 0
fi

if [ ! -d "$TEST_PROJECT" ]; then
    echo "Test project not found: $TEST_PROJECT (set TEST_PROJECT to override)" >&2
    exit 1
fi

addon_dest="$TEST_PROJECT/addons/godot-neovim"
echo ">> Deploying to $addon_dest"
mkdir -p "$addon_dest/bin/$bin_dir" "$addon_dest/lua/godot_neovim" "$addon_dest/input"
cp "$artifact" "$addon_dest/bin/$bin_dir/"
cp "$repo_root"/addons/godot-neovim/lua/godot_neovim/*.lua "$addon_dest/lua/godot_neovim/"
cp "$repo_root"/addons/godot-neovim/input/*.gd "$addon_dest/input/"

echo "Done: $PROFILE build deployed to $TEST_PROJECT"
