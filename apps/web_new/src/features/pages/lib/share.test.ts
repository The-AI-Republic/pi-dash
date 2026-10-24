// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row copy-link helper (PAGE-006/010): canonical address and notices.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toaster = vi.hoisted(() => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => toaster);

import { copyPageLink, PAGE_LINK_COPIED_TITLE, PAGE_LINK_COPY_FAILED_TITLE, pageLinkUrl } from "./share.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PAGE = "323e4567-e89b-12d3-a456-426614174002";

beforeEach(() => {
  toaster.notifySuccess.mockReset();
  toaster.notifyError.mockReset();
  Object.defineProperty(window, "location", {
    configurable: true,
    value: { origin: "https://app.test" },
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("copyPageLink", () => {
  it("writes the canonical detail address and reports success", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    await expect(copyPageLink("acme", PROJECT, PAGE)).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith(`https://app.test/acme/projects/${PROJECT}/pages/${PAGE}`);
    expect(toaster.notifySuccess).toHaveBeenCalledWith(PAGE_LINK_COPIED_TITLE);
    expect(toaster.notifyError).not.toHaveBeenCalled();
  });

  it("reports a refused clipboard write", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    await expect(copyPageLink("acme", PROJECT, PAGE)).resolves.toBe(false);
    expect(toaster.notifyError).toHaveBeenCalledWith(PAGE_LINK_COPY_FAILED_TITLE);
    expect(pageLinkUrl("acme", PROJECT, PAGE)).toBe(`https://app.test/acme/projects/${PROJECT}/pages/${PAGE}`);
  });
});
