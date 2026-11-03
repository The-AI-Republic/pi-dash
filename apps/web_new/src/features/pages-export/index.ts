// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the pages-export feature (NEWFRONT-770, EXP-016–021). The
// Pages detail mounts the dialog through the registration seam; the
// renderers and their helpers stay available for tests and reuse.
export { PageExportDialog } from "./components/PageExportDialog.js";
export type { PageExportDialogProps } from "./components/PageExportDialog.js";
export { browserPageExportSink } from "./lib/downloadSink.js";
export type { PageExportSink } from "./lib/downloadSink.js";
export { extractExportContent } from "./lib/extractExportContent.js";
export type { ExtractedExportContent, ExtractExportContentOptions } from "./lib/extractExportContent.js";
export { browserFetchExportImageBytes, fetchExportImages } from "./lib/fetchExportImages.js";
export type { FetchExportImageBytes } from "./lib/fetchExportImages.js";
export { applyMarkdownScope, stripMarkdownImages } from "./lib/markdownExport.js";
export {
  getPageExportDialog,
  registerPageExportDialog,
  resetPageExportDialog,
  usePageExportDialog,
} from "./lib/pageExportDialogRegistry.js";
export type { PageExportDialogComponent, PageExportHost } from "./lib/pageExportDialogRegistry.js";
export {
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
} from "./lib/pageExportModel.js";
export type { PageExportForm, PageExportFormat, PageExportScope, PageExportSize } from "./lib/pageExportModel.js";
export {
  PAGE_PDF_FONT_FAMILY,
  loadInterFontBytes,
  pagePdfSize,
  renderPagePdf,
  toExportImageDataUri,
} from "./lib/renderPagePdf.js";
export type { LoadInterFontBytes, PagePdfSize, RenderPagePdfDeps, RenderPagePdfInput } from "./lib/renderPagePdf.js";
export { savePageExport } from "./lib/savePageExport.js";
export type { SavePageExportDeps, SavedPageExport } from "./lib/savePageExport.js";
