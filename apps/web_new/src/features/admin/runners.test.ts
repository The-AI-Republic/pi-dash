// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners page pure helpers: filter options, cell formatting, the
// total line, and the pager math.
import { describe, expect, it } from "vitest";

import {
  formatProvisioningOptionLabel,
  formatRunnerCellText,
  formatRunnerHeartbeat,
  formatRunnerTotal,
  formatRunnersPageLabel,
  RUNNERS_FILTER_OPTIONS,
  RUNNERS_HEADERS,
  runnersPageCount,
} from "./runners.js";

describe("runners helpers", () => {
  it("lists the eight table headers in column order", () => {
    expect(RUNNERS_HEADERS).toEqual([
      "Name",
      "Workspace",
      "Owner",
      "Provisioning",
      "Status",
      "Runner version",
      "Codex version",
      "Last heartbeat",
    ]);
  });

  it("offers the blank filter first, then the known provisioning values", () => {
    expect(RUNNERS_FILTER_OPTIONS).toEqual(["", "manual", "desktop_bundled"]);
  });

  it("labels the blank provisioning option as all", () => {
    expect(formatProvisioningOptionLabel("")).toBe("all");
    expect(formatProvisioningOptionLabel("manual")).toBe("manual");
    expect(formatProvisioningOptionLabel("desktop_bundled")).toBe("desktop_bundled");
  });

  it("renders set cell text as-is and absent text as an em dash", () => {
    expect(formatRunnerCellText("acme")).toBe("acme");
    expect(formatRunnerCellText(null)).toBe("—");
    expect(formatRunnerCellText("")).toBe("—");
  });

  it("renders the heartbeat in locale format and an absent one as an em dash", () => {
    expect(formatRunnerHeartbeat("2026-01-02T03:04:05+00:00")).toBe(
      new Date("2026-01-02T03:04:05+00:00").toLocaleString()
    );
    expect(formatRunnerHeartbeat(null)).toBe("—");
  });

  it("pluralizes the total-count line", () => {
    expect(formatRunnerTotal(0)).toBe("0 runners");
    expect(formatRunnerTotal(1)).toBe("1 runner");
    expect(formatRunnerTotal(51)).toBe("51 runners");
  });

  it("counts pages at fifty rows per page", () => {
    expect(runnersPageCount(0)).toBe(0);
    expect(runnersPageCount(1)).toBe(1);
    expect(runnersPageCount(50)).toBe(1);
    expect(runnersPageCount(51)).toBe(2);
    expect(runnersPageCount(100)).toBe(2);
  });

  it("labels the pager with the current page number", () => {
    expect(formatRunnersPageLabel(1)).toBe("Page 1");
    expect(formatRunnersPageLabel(3)).toBe("Page 3");
  });
});
