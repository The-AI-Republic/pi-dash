// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The one copy-link path every screen uses (DESK-019): the shared helper
// writes the platform's shareable address and turns a refused clipboard or
// a missing hosted origin into the failure toast.

import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { platform } from "../../core/platform/index.js";
import { copyLinkWithToast, copyShareAddress, copyWorkItemLink, shareableAddress, workItemPath } from "./shareLink.js";
import { notifyError, notifySuccess } from "./toaster.js";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
});

describe("share link helper", () => {
  it("builds the canonical work-item path", () => {
    expect(workItemPath("acme", "ACME-12")).toBe("/acme/browse/ACME-12/");
  });

  it("asks the active platform for the address", () => {
    const shareableUrl = vi.spyOn(platform, "shareableUrl").mockReturnValue("https://app.example.test/acme/");
    expect(shareableAddress("/acme/")).toBe("https://app.example.test/acme/");
    expect(shareableUrl).toHaveBeenCalledWith("/acme/");
  });

  it("writes the address through the clipboard", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue("https://app.example.test/acme/browse/ACME-12/");
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await expect(copyShareAddress("/acme/browse/ACME-12/")).resolves.toBe(
      "https://app.example.test/acme/browse/ACME-12/"
    );
    expect(writeText).toHaveBeenCalledWith("https://app.example.test/acme/browse/ACME-12/");
  });

  it("rejects when the window has no clipboard", async () => {
    vi.spyOn(platform, "shareableUrl").mockReturnValue("https://app.example.test/acme/");
    vi.stubGlobal("navigator", {});
    await expect(copyShareAddress("/acme/")).rejects.toThrow("clipboard");
  });

  it("toasts success and failure without rejecting", async () => {
    const notify = { success: vi.fn(), error: vi.fn() };
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    vi.spyOn(platform, "shareableUrl").mockReturnValue("https://app.example.test/acme/");
    await expect(copyLinkWithToast("/acme/", { copied: "Copied", failed: "Failed" }, notify)).resolves.toBe(true);
    expect(notify.success).toHaveBeenCalledWith("Copied", undefined);

    vi.spyOn(platform, "shareableUrl").mockImplementation(() => {
      throw new Error("A hosted web origin is required to create a shareable link.");
    });
    await expect(copyLinkWithToast("/acme/", { copied: "Copied", failed: "Failed" }, notify)).resolves.toBe(false);
    expect(notify.error).toHaveBeenCalledWith("Failed", "A hosted web origin is required to create a shareable link.");
  });

  it("reports a copy through the app toasts", async () => {
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    vi.spyOn(platform, "shareableUrl").mockReturnValue("https://app.example.test/acme/");
    const messages = { copied: "Copied", copiedBody: "Body", failed: "Failed", failedBody: "Try again" };
    await expect(copyWorkItemLink("/acme/", messages)).resolves.toBe(true);
    expect(notifySuccess).toHaveBeenCalledWith("Copied", "Body");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reports a refused copy through the app toasts", async () => {
    vi.stubGlobal("navigator", {});
    vi.spyOn(platform, "shareableUrl").mockReturnValue("https://app.example.test/acme/");
    const messages = { copied: "Copied", failed: "Failed", failedBody: "Try again" };
    await expect(copyWorkItemLink("/acme/", messages)).resolves.toBe(false);
    expect(notifyError).toHaveBeenCalledWith("Failed", "Try again");
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("reports a non-Error refusal without inventing a body", async () => {
    const notify = { success: vi.fn(), error: vi.fn() };
    vi.spyOn(platform, "shareableUrl").mockReturnValue("https://app.example.test/acme/");
    vi.stubGlobal("navigator", {
      clipboard: {
        writeText: async () => {
          throw "denied";
        },
      },
    });
    await expect(copyLinkWithToast("/acme/", { copied: "Copied", failed: "Failed" }, notify)).resolves.toBe(false);
    expect(notify.error).toHaveBeenCalledWith("Failed", undefined);
  });
});
