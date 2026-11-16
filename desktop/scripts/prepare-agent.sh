#!/usr/bin/env bash
# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Stage the CLI the desktop app bundles (`pidash` only): by default, download
# the latest released `pidash-v*` binary for the target platform (no cargo
# build of the CLI). A dirty or unreleased checkout can never reach users
# through the desktop app.
#
# Building the CLI from source is only available as an explicit opt-in for
# CLI development: PIDASH_BUNDLE_CLI_FROM_SOURCE=1 (with a loud warning).
#
# The built-in engine is owned by the CLI release (`engine/<version>/`
# installed by `pidash engine install`); the desktop bundle carries only the
# CLI. There is no engine download, marker, or licence staging here.
set -euo pipefail
desktop_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
oss_dir="$(cd -- "$desktop_dir/.." && pwd)"
bin_dir="$desktop_dir/src-tauri/bin"
license_dir="$desktop_dir/src-tauri/licenses"
mkdir -p "$bin_dir" "$license_dir"

# Target platform: TAURI_ENV_TARGET_TRIPLE when set (what `tauri build
# --target <triple>` exports), otherwise the host triple. A cross-target
# build must bundle the *target* binary, not the host's. macOS always ships
# universal (lipo of arm64 + x86_64 slices).
tauri_triple="${TAURI_ENV_TARGET_TRIPLE:-}"
if [[ -n "$tauri_triple" ]]; then
  target="$tauri_triple"
else
  target="$(rustc -vV | sed -n 's/^host: //p')"
fi
case "$target" in
  aarch64-apple-darwin|x86_64-apple-darwin) target="universal-apple-darwin" ;;
esac
case "$target" in
  universal-apple-darwin|*-unknown-linux-gnu|*-unknown-linux-musl|*-apple-darwin|*-pc-windows-msvc) ;;
  *) echo "Unsupported desktop build target: $target" >&2; exit 1 ;;
esac

from_source="${PIDASH_BUNDLE_CLI_FROM_SOURCE:-0}"
if [[ "$from_source" == "1" ]]; then
  echo "[prepare-agent] WARNING: PIDASH_BUNDLE_CLI_FROM_SOURCE=1 — building the bundled CLI from this checkout with cargo instead of downloading the latest released pidash-v* binary. Do not ship this build." >&2
  # dev-prep.sh runs this for both beforeDevCommand and beforeBuildCommand, and
  # whatever lands in src-tauri/bin/ is sealed into the bundle as the daemon the
  # app spawns, so the default profile follows the build being served. The Tauri
  # CLI always exports TAURI_ENV_PLATFORM to its hooks, and TAURI_ENV_DEBUG=true
  # only for `tauri dev` / `tauri build --debug` (2.10 leaves it unset for a
  # release build rather than setting it to false). Run by hand, default to dev.
  default_profile=dev
  if [[ -n "${TAURI_ENV_PLATFORM:-}" && "${TAURI_ENV_DEBUG:-}" != true ]]; then
    default_profile=release
  fi
  runner_profile="${PIDASH_RUNNER_PROFILE:-$default_profile}"
  if [[ "$default_profile" == release && "$runner_profile" == dev ]]; then
    echo "[prepare-agent] WARNING: PIDASH_RUNNER_PROFILE=dev in a release build — the bundle will ship an unoptimized debug runner." >&2
  fi
  echo "[prepare-agent] Building the bundled runner with cargo profile: $runner_profile (target $target)"
  output_profile="$runner_profile"
  if [[ "$runner_profile" == dev ]]; then output_profile=debug; fi
  target_dir="${CARGO_TARGET_DIR:-$oss_dir/target}"
  suffix=""
  case "$target" in
    universal-apple-darwin) suffix="" ;;
    *-pc-windows-msvc) suffix=".exe" ;;
    *) suffix="" ;;
  esac
  mkdir -p "$bin_dir"

  build_one() {
    local arch="$1"
    local sfx=""
    case "$arch" in *-pc-windows-msvc) sfx=".exe" ;; esac
    cargo build --manifest-path "$oss_dir/Cargo.toml" --profile "$runner_profile" --bin pidash --target "$arch"
    local src="$target_dir/$arch/$output_profile/pidash$sfx"
    if [[ ! -f "$src" ]]; then
      echo "[prepare-agent] ERROR: built pidash not found at $src (CARGO_TARGET_DIR=$target_dir, target=$arch)" >&2
      exit 1
    fi
    printf '%s' "$src"
  }

  if [[ "$target" == "universal-apple-darwin" ]]; then
    arm_src="$(build_one aarch64-apple-darwin)"
    x86_src="$(build_one x86_64-apple-darwin)"
    lipo -create "$arm_src" "$x86_src" -output "$bin_dir/pidash"
    lipo "$bin_dir/pidash" -verify_arch arm64 x86_64
    chmod 755 "$bin_dir/pidash"
  else
    # Single-arch target: cargo already placed it at target/<triple>/<profile>/
    cargo build --manifest-path "$oss_dir/Cargo.toml" --profile "$runner_profile" --bin pidash --target "$target"
    src="$target_dir/$target/$output_profile/pidash$suffix"
    if [[ ! -f "$src" ]]; then
      # Fallback for a host build that escaped --target (should not happen)
      src="$target_dir/$output_profile/pidash$suffix"
    fi
    if [[ ! -f "$src" ]]; then
      echo "[prepare-agent] ERROR: built pidash not found at $src (CARGO_TARGET_DIR=$target_dir, target=$target)" >&2
      exit 1
    fi
    cp "$src" "$bin_dir/pidash$suffix"
  fi

  cp "$oss_dir/LICENSE.txt" "$license_dir/pidash-LICENSE.txt"
  # Record the bundled version (from runner/Cargo.toml's version field) so the
  # app can compare at runtime without running the binary.
  version=""
  if [[ -f "$oss_dir/runner/Cargo.toml" ]]; then
    version="$(grep -E '^[[:space:]]*version[[:space:]]*=' "$oss_dir/runner/Cargo.toml" | head -n1 | sed -E 's/.*"?([0-9]+\.[0-9]+\.[0-9]+[^"]*)"?/\1/' || true)"
  fi
  if [[ -z "$version" || "$version" == *version* ]]; then
    version="$(grep -E '^[[:space:]]*version[[:space:]]*=' "$oss_dir/runner/Cargo.toml" 2>/dev/null | head -n1 | cut -d'"' -f2 || echo "0.0.0-dev")"
  fi
  printf '%s\n' "$version" > "$bin_dir/pidash.version"
  echo "[prepare-agent] From-source CLI version $version -> $bin_dir/pidash.version"
  exit 0
fi

# Default: download the released CLI for the target platform. The resolved tag
# (latest stable pidash-v* unless PIDASH_BUNDLE_VERSION pins it) is printed by
# bundle_agents.py and in the run summary.
bundle_py="$desktop_dir/scripts/bundle_agents.py"
if [[ ! -f "$bundle_py" ]]; then
  echo "[prepare-agent] ERROR: bundle_agents.py not found at $bundle_py" >&2
  exit 1
fi
echo "[prepare-agent] Downloading released CLI for target $target (no cargo build)"
# Direct stdout log so the build operator sees the tag even if bundle_agents.py
# is stubbed; bundle_agents.py itself also prints the resolved tag.
runner_tag_arg=""
if [[ -n "${PIDASH_BUNDLE_VERSION:-}" ]]; then
  echo "[prepare-agent] Using PIDASH_BUNDLE_VERSION=$PIDASH_BUNDLE_VERSION"
  runner_tag_arg="--runner-tag $PIDASH_BUNDLE_VERSION"
fi
# shellcheck disable=SC2086
python3 "$bundle_py" --target "$target" --output "$bin_dir" $runner_tag_arg
cp "$oss_dir/LICENSE.txt" "$license_dir/pidash-LICENSE.txt"
