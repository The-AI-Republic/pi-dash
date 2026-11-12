// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the pages-export feature (NEWFRONT-770).
import { describe, expect, it } from "vitest";

import {
  applyMarkdownScope,
  browserFetchExportImageBytes,
  browserPageExportSink,
  DEFAULT_PAGE_EXPORT_FORMAT,
  DEFAULT_PAGE_EXPORT_SCOPE,
  DEFAULT_PAGE_EXPORT_SIZE,
  defaultPageExportForm,
  extractExportContent,
  fetchExportImages,
  getPageExportDialog,
  isPageExportSizeVisible,
  loadInterFontBytes,
  PAGE_EXPORT_FORMATS,
  PAGE_EXPORT_SCOPES,
  PAGE_EXPORT_SIZES,
  PAGE_PDF_FONT_FAMILY,
  PageExportDialog,
  pageExportMarkdownFilename,
  pageExportPdfFilename,
  pageExportTitleSlug,
  pagePdfSize,
  registerPageExportDialog,
  renderPagePdf,
  resetPageExportDialog,
  savePageExport,
  stripMarkdownImages,
  toExportImageDataUri,
  usePageExportDialog,
} from "./index.js";

describe("pages-export barrel", () => {
  it("re-exports the dialog, seam and renderers", () => {
    expect(PageExportDialog).toBeDefined();
    expect(registerPageExportDialog).toBeDefined();
    expect(getPageExportDialog).toBeDefined();
    expect(usePageExportDialog).toBeDefined();
    expect(resetPageExportDialog).toBeDefined();
    expect(savePageExport).toBeDefined();
    expect(renderPagePdf).toBeDefined();
    expect(loadInterFontBytes).toBeDefined();
    expect(toExportImageDataUri).toBeDefined();
    expect(PAGE_PDF_FONT_FAMILY).toBe("Inter");
    expect(pagePdfSize).toBeDefined();
    expect(extractExportContent).toBeDefined();
    expect(fetchExportImages).toBeDefined();
    expect(browserFetchExportImageBytes).toBeDefined();
    expect(browserPageExportSink).toBeDefined();
    expect(applyMarkdownScope).toBeDefined();
    expect(stripMarkdownImages).toBeDefined();
    expect(defaultPageExportForm).toBeDefined();
    expect(isPageExportSizeVisible).toBeDefined();
    expect(pageExportTitleSlug).toBeDefined();
    expect(pageExportPdfFilename).toBeDefined();
    expect(pageExportMarkdownFilename).toBeDefined();
    expect(DEFAULT_PAGE_EXPORT_FORMAT).toBe("PDF");
    expect(DEFAULT_PAGE_EXPORT_SCOPE).toBe("Everything");
    expect(DEFAULT_PAGE_EXPORT_SIZE).toBe("A4");
    expect(PAGE_EXPORT_FORMATS).toContain("PDF");
    expect(PAGE_EXPORT_SCOPES).toContain("Everything");
    expect(PAGE_EXPORT_SIZES).toContain("A4");
  });
});
