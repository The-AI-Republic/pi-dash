#!/usr/bin/env bash
# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Stage the CLI the desktop app bundles (`pidash` only): build this
# checkout's runner and copy it into `src-tauri/bin/`, with its licence.
set -euo pipefail
desktop_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
oss_dir="$(cd -- "$desktop_dir/.." && pwd)"
# The built-in engine is owned by the CLI release (`engine/<version>/`
# installed by `pidash engine install`); the desktop bundle carries only the
# CLI. There is no engine download, marker, or licence staging here.
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
echo "[prepare-agent] Building the bundled runner with cargo profile: $runner_profile"
target="$(rustc -vV | sed -n 's/^host: //p')"
suffix=""
case "$target" in
  *-unknown-linux-gnu|*-apple-darwin) ;;
  *-pc-windows-msvc) suffix=".exe" ;;
  *) echo "Unsupported desktop build target: $target" >&2; exit 1 ;;
esac
bin_dir="$desktop_dir/src-tauri/bin"
license_dir="$desktop_dir/src-tauri/licenses"
mkdir -p "$bin_dir" "$license_dir"
cargo build --manifest-path "$oss_dir/Cargo.toml" --profile "$runner_profile" --bin pidash
output_profile="$runner_profile"
if [[ "$runner_profile" == dev ]]; then output_profile=debug; fi
cp "$oss_dir/target/$output_profile/pidash$suffix" "$bin_dir/pidash$suffix"
cp "$oss_dir/LICENSE.txt" "$license_dir/pidash-LICENSE.txt"
