// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export dialog model (parity/specs/exports.md §5, EXP-016/017/019/020):
// defaults, option order, size-row visibility, and the file-name rules.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_PAGE_EXPORT_FORMAT,
  DEFAULT_PAGE_EXPORT_SCOPE,
  DEFAULT_PAGE_EXPORT_SIZE,
  PAGE_EXPORT_FORMATS,
  PAGE_EXPORT_SCOPES,
  PAGE_EXPORT_SIZES,
  defaultPageExportForm,
  isPageExportSizeVisible,
  pageExportMarkdownFilename,
  pageExportPdfFilename,
  pageExportTitleSlug,
} from "./pageExportModel.js";

describe("page export defaults and options", () => {
  it("defaults to PDF / Everything / A4", () => {
    expect(defaultPageExportForm()).toEqual({ format: "PDF", scope: "Everything", size: "A4" });
    expect(DEFAULT_PAGE_EXPORT_FORMAT).toBe("PDF");
    expect(DEFAULT_PAGE_EXPORT_SCOPE).toBe("Everything");
    expect(DEFAULT_PAGE_EXPORT_SIZE).toBe("A4");
  });

  it("offers the options in dialog order", () => {
    expect([...PAGE_EXPORT_FORMATS]).toEqual(["PDF", "Markdown"]);
    expect([...PAGE_EXPORT_SCOPES]).toEqual(["Everything", "No images"]);
    expect([...PAGE_EXPORT_SIZES]).toEqual(["A4", "A3", "A2", "Letter", "Legal", "Tabloid"]);
  });

  it("shows the size row only for the document format", () => {
    expect(isPageExportSizeVisible("PDF")).toBe(true);
    expect(isPageExportSizeVisible("Markdown")).toBe(false);
  });
});

describe("page export title slug", () => {
  it("lowercases and dash-collapses like the oracle helper", () => {
    expect(pageExportTitleSlug("Doc019 abc123")).toBe("doc019-abc123");
    expect(pageExportTitleSlug("Hello,  World!")).toBe("hello-world-");
    expect(pageExportTitleSlug("under_score-kept")).toBe("under_score-kept");
    expect(pageExportTitleSlug("  padded  ")).toBe("-padded-");
  });
});

describe("page export file names", () => {
  it("names documents after the slug and the lowercased size", () => {
    expect(pageExportPdfFilename("Doc019 abc123", "A4")).toBe("doc019-abc123-a4.pdf");
    expect(pageExportPdfFilename("Doc019 abc123", "Letter")).toBe("doc019-abc123-letter.pdf");
    expect(pageExportPdfFilename("Hello, World!", "Tabloid")).toBe("hello-world--tabloid.pdf");
  });

  it("names plain text after the slug alone", () => {
    expect(pageExportMarkdownFilename("Text020 abc123")).toBe("text020-abc123.md");
  });
});
