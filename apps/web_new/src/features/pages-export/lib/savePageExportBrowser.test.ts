// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export browser defaults (EXP-020/021): with no injected surface the
// writer saves through the live browser primitives and the DOM extractor.
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PageExportHost } from "./pageExportDialogRegistry.js";
import { savePageExport } from "./savePageExport.js";

function host(): PageExportHost {
  return {
    pageTitle: "Text020 probe",
    projectId: "project-1",
    workspaceSlug: "acme",
    getHtml: () => "<p>body marker probe</p>",
    getMarkdown: () => "body marker probe\n",
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("savePageExport browser defaults", () => {
  it("saves plain text through the browser surface and DOM extractor", async () => {
    Object.defineProperty(URL, "createObjectURL", { value: () => "blob:browser", configurable: true, writable: true });
    const revoked: string[] = [];
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
    const text = await savePageExport(
      host(),
      { format: "Markdown", scope: "Everything", size: "A4" },
      {
        origin: "https://app.example",
      }
    );
    expect(text.filename).toBe("text020-probe.md");
    expect(clicks).toEqual(["blob:browser text020-probe.md"]);
    expect(revoked).toEqual(["blob:browser"]);
  }, 60_000);
});
