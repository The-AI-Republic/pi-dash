// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The one copy helper: the canonical detail address lands on the
// clipboard with the pinned confirmation, a refusal toasts the failure
// without claiming success, and opening in a new tab uses the
// platform's external opener.

import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { platform } from "../../../core/platform/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { MODULE_FAILURE_TITLE, MODULE_SUCCESS_TITLE } from "./moduleForm.js";
import {
  copyModuleLink,
  MODULE_LINK_COPIED_BODY,
  MODULE_OPEN_NEW_TAB_FAILED_TITLE,
  openModuleInNewTab,
} from "./moduleShare.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const PATH = `/${WS}/projects/${PROJECT}/modules/${MODULE}`;
const ADDRESS = `https://app.example.test${PATH}`;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
});

describe("module share helper", () => {
  it("copies the canonical detail address with the pinned confirmation", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await expect(copyModuleLink(WS, PROJECT, MODULE)).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith(ADDRESS);
    expect(notifySuccess).toHaveBeenCalledWith(MODULE_SUCCESS_TITLE, MODULE_LINK_COPIED_BODY);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reports a refused copy as a failure without claiming success", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    vi.stubGlobal("navigator", {});
    await expect(copyModuleLink(WS, PROJECT, MODULE)).resolves.toBe(false);
    expect(notifyError).toHaveBeenCalledWith(MODULE_FAILURE_TITLE, expect.stringContaining("clipboard"));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("opens the canonical address in a new tab through the platform", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    const openExternal = vi.spyOn(platform, "openExternal").mockResolvedValue(undefined);
    await openModuleInNewTab(WS, PROJECT, MODULE);
    expect(openExternal).toHaveBeenCalledWith(ADDRESS);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts when the new tab refuses to open", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    vi.spyOn(platform, "openExternal").mockRejectedValue(new Error("blocked"));
    await openModuleInNewTab(WS, PROJECT, MODULE);
    expect(notifyError).toHaveBeenCalledWith(MODULE_OPEN_NEW_TAB_FAILED_TITLE, "blocked");
  });

  it("toasts without a detail when the refusal carries none", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue(ADDRESS);
    vi.spyOn(platform, "openExternal").mockRejectedValue("blocked");
    await openModuleInNewTab(WS, PROJECT, MODULE);
    expect(notifyError).toHaveBeenCalledWith(MODULE_OPEN_NEW_TAB_FAILED_TITLE, undefined);
  });
});
