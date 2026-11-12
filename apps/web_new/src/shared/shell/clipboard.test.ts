// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./toaster.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { platform } from "../../core/platform/index.js";
import { PAGE_URL_COPIED_TITLE, PAGE_URL_COPY_FAILED_TITLE, copyCurrentPageUrl } from "./clipboard.js";
import { notifyError, notifySuccess } from "./toaster.js";

afterEach(() => {
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
  vi.unstubAllGlobals();
});

describe("copy current page URL", () => {
  it("copies the address with a confirming toast", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await copyCurrentPageUrl();
    expect(writeText).toHaveBeenCalledWith(window.location.href);
    expect(notifySuccess).toHaveBeenCalledWith(PAGE_URL_COPIED_TITLE);
  });

  it("toasts a denied write", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await copyCurrentPageUrl();
    expect(notifyError).toHaveBeenCalledWith(PAGE_URL_COPY_FAILED_TITLE, "denied");
  });

  it("copies the hosted address on the desktop bundle, keeping query and fragment", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    vi.stubGlobal("window", {
      location: {
        origin: "http://tauri.localhost",
        href: "http://tauri.localhost/acme/browse/ACME-1/?pane=info#comment-3",
        pathname: "/acme/browse/ACME-1/",
        search: "?pane=info",
        hash: "#comment-3",
      },
    });
    vi.spyOn(platform, "shareableUrl").mockImplementation((path: string) => `https://app.example.test${path}`);
    await copyCurrentPageUrl();
    expect(writeText).toHaveBeenCalledWith("https://app.example.test/acme/browse/ACME-1/?pane=info#comment-3");
    expect(notifySuccess).toHaveBeenCalledWith(PAGE_URL_COPIED_TITLE);
  });

  it("toasts a refused shareable link instead of writing a bundle address", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    vi.stubGlobal("window", {
      location: {
        origin: "http://tauri.localhost",
        href: "http://tauri.localhost/acme/",
        pathname: "/acme/",
        search: "",
        hash: "",
      },
    });
    vi.spyOn(platform, "shareableUrl").mockImplementation(() => {
      throw new Error("A hosted web origin is required to create a shareable link.");
    });
    await copyCurrentPageUrl();
    expect(writeText).not.toHaveBeenCalled();
    expect(notifyError).toHaveBeenCalledWith(
      PAGE_URL_COPY_FAILED_TITLE,
      "A hosted web origin is required to create a shareable link."
    );
  });
});
