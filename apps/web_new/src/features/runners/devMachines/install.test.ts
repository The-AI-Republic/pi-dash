// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { INSTALL_BLOCKS, INSTALL_PREREQUISITE, WINDOWS_MSI_URL } from "./install.js";

describe("install catalogue", () => {
  it("has the three platform blocks in order", () => {
    expect(INSTALL_BLOCKS.map((block) => block.label)).toEqual([
      "macOS / Linux",
      "Windows (PowerShell)",
      "Windows (MSI)",
    ]);
  });

  it("points every block at the latest release channel", () => {
    expect(INSTALL_BLOCKS[0]?.command).toContain("install.sh");
    expect(INSTALL_BLOCKS[1]?.command).toContain("install.ps1");
    expect(INSTALL_BLOCKS[2]?.command).toContain("msi");
    expect(INSTALL_BLOCKS[0]?.download).toBeNull();
    expect(INSTALL_BLOCKS[1]?.download).toBeNull();
    expect(INSTALL_BLOCKS[2]?.download).toBe(WINDOWS_MSI_URL);
    expect(WINDOWS_MSI_URL).toContain("releases/latest/download");
    expect(WINDOWS_MSI_URL.endsWith(".msi")).toBe(true);
  });

  it("states the PATH and doctor prerequisite", () => {
    expect(INSTALL_PREREQUISITE).toContain("PATH");
    expect(INSTALL_PREREQUISITE).toContain("doctor");
  });
});
