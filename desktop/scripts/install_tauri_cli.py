# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Install the pinned official Tauri CLI after verifying its release digest."""

import argparse
import os
from pathlib import Path

from bundle_agents import download, executable_bytes

TAG = "tauri-cli-v2.11.4"
TARGETS = {
    ("Linux", "X64"): "x86_64-unknown-linux-gnu",
    ("macOS", "ARM64"): "aarch64-apple-darwin",
    ("macOS", "X64"): "x86_64-apple-darwin",
    ("Windows", "X64"): "x86_64-pc-windows-msvc",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    runner_os = os.environ["RUNNER_OS"]
    target = TARGETS[(runner_os, os.environ["RUNNER_ARCH"])]
    extension = "tgz" if runner_os == "Linux" else "zip"
    archive = f"cargo-tauri-{target}.{extension}"
    binary = "cargo-tauri.exe" if runner_os == "Windows" else "cargo-tauri"
    payload = executable_bytes(download("tauri-apps/tauri", TAG, archive), archive, binary)
    args.output.mkdir(parents=True, exist_ok=True)
    destination = args.output / binary
    destination.write_bytes(payload)
    destination.chmod(0o755)
    print(f"Installed {TAG} for {target} at {destination}")


if __name__ == "__main__":
    main()
