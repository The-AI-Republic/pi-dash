// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export download surface: the browser sink round-trips a blob URL
// and clicks an anchor without leaving it behind.
import { afterEach, describe, expect, it, vi } from "vitest";

import { browserPageExportSink } from "./downloadSink.js";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("browserPageExportSink", () => {
  it("saves through an object URL and cleans up", () => {
    const created: Blob[] = [];
    const revoked: string[] = [];
    Object.defineProperty(URL, "createObjectURL", {
      value: (blob: Blob) => {
        created.push(blob);
        return "blob:page-export";
      },
      configurable: true,
      writable: true,
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      value: (url: string) => {
        revoked.push(url);
      },
      configurable: true,
      writable: true,
    });
    const clicks: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      clicks.push(`${this.href} ${this.download}`);
    });

    const sink = browserPageExportSink();
    const blob = new Blob(["%PDF-bytes"], { type: "application/pdf" });
    const url = sink.createObjectUrl(blob);
    sink.save(url, "page-a4.pdf");
    sink.revokeObjectUrl(url);
    expect(created).toEqual([blob]);
    expect(clicks).toEqual(["blob:page-export page-a4.pdf"]);
    expect(revoked).toEqual(["blob:page-export"]);
    expect(document.querySelector("a[download]")).toBeNull();
  });
});
