// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The one copy helper: the canonical detail address lands on the
// clipboard with confirmation, a refusal toasts the failure, and
// opening in a new tab uses the platform's external opener.

import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { platform } from "../../../core/platform/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import {
  CYCLE_LINK_COPIED_TITLE,
  CYCLE_LINK_COPY_FAILED_TITLE,
  CYCLE_OPEN_NEW_TAB_FAILED_TITLE,
  copyCycleLink,
  openCycleInNewTab,
} from "./cycleShare.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const PATH = `/${WS}/projects/${PROJECT}/cycles/${CYCLE}`;
const ADDRESS = `https://app.example.test${PATH}`;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
});

describe("cycle share helper", () => {
  it("copies the canonical detail address with confirmation", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await expect(copyCycleLink(WS, PROJECT, CYCLE)).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith(ADDRESS);
    expect(notifySuccess).toHaveBeenCalledWith(CYCLE_LINK_COPIED_TITLE, undefined);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reports a refused copy as a failure without claiming success", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    vi.stubGlobal("navigator", {});
    await expect(copyCycleLink(WS, PROJECT, CYCLE)).resolves.toBe(false);
    expect(notifyError).toHaveBeenCalledWith(CYCLE_LINK_COPY_FAILED_TITLE, expect.stringContaining("clipboard"));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("opens the canonical address in a new tab through the platform", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    const openExternal = vi.spyOn(platform, "openExternal").mockResolvedValue(undefined);
    await openCycleInNewTab(WS, PROJECT, CYCLE);
    expect(openExternal).toHaveBeenCalledWith(ADDRESS);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts a refused open instead of throwing", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    vi.spyOn(platform, "openExternal").mockRejectedValue(new Error("no bridge"));
    await expect(openCycleInNewTab(WS, PROJECT, CYCLE)).resolves.toBeUndefined();
    expect(notifyError).toHaveBeenCalledWith(CYCLE_OPEN_NEW_TAB_FAILED_TITLE, "no bridge");
  });

  it("toasts a refusal without a reason when the opener gives none", async () => {
    vi.spyOn(platform, "shareableUrl").mockImplementation(() => {
      throw "offline";
    });
    await expect(openCycleInNewTab(WS, PROJECT, CYCLE)).resolves.toBeUndefined();
    expect(notifyError).toHaveBeenCalledWith(CYCLE_OPEN_NEW_TAB_FAILED_TITLE, undefined);
  });
});
