// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export file writer (parity/specs/exports.md §5, EXP-019–021): render
// the host's live content to the chosen format and hand the bytes to the
// download surface. Fully client-side: no export endpoint is ever called.
import { applyMarkdownScope } from "./markdownExport.js";
import { browserPageExportSink, type PageExportSink } from "./downloadSink.js";
import {
  extractExportContent,
  type ExtractedExportContent,
  type ExtractExportContentOptions,
} from "./extractExportContent.js";
import { browserFetchExportImageBytes, fetchExportImages, type FetchExportImageBytes } from "./fetchExportImages.js";
import type { PageExportHost } from "./pageExportDialogRegistry.js";
import { pageExportMarkdownFilename, pageExportPdfFilename, type PageExportForm } from "./pageExportModel.js";
import { renderPagePdf, type LoadInterFontBytes } from "./renderPagePdf.js";

export interface SavePageExportDeps {
  sink?: PageExportSink | undefined;
  /** Page origin that stack-asset picture URLs resolve against. */
  origin: string;
  fetchBytes?: FetchExportImageBytes | undefined;
  /** Inter TTF bytes; the production loader fetches them when absent. */
  fontBytes?: Uint8Array | undefined;
  loadFontBytes?: LoadInterFontBytes | undefined;
  /** Editor-HTML reader; the DOM extractor unless a test stands in. */
  extractContent?: ((html: string, options?: ExtractExportContentOptions) => ExtractedExportContent) | undefined;
}

export interface SavedPageExport {
  filename: string;
}

/** Render the page in the chosen format and save it through the sink. */
export async function savePageExport(
  host: PageExportHost,
  form: PageExportForm,
  deps: SavePageExportDeps
): Promise<SavedPageExport> {
  const sink = deps.sink ?? browserPageExportSink();
  const extract = deps.extractContent ?? extractExportContent;
  let filename: string;
  let blob: Blob;
  if (form.format === "Markdown") {
    filename = pageExportMarkdownFilename(host.pageTitle);
    blob = new Blob([applyMarkdownScope(host.getMarkdown(), form.scope)], { type: "text/markdown" });
  } else {
    const content = extract(host.getHtml(), { workspaceSlug: host.workspaceSlug, projectId: host.projectId });
    const imageUrls = form.scope === "No images" ? [] : content.imageUrls;
    const imageBytes = await fetchExportImages(imageUrls, deps.origin, deps.fetchBytes ?? browserFetchExportImageBytes);
    const document = await renderPagePdf(
      { title: host.pageTitle, paragraphs: content.paragraphs, imageBytes, size: form.size },
      { fontBytes: deps.fontBytes, loadFontBytes: deps.loadFontBytes }
    );
    filename = pageExportPdfFilename(host.pageTitle, form.size);
    blob = new Blob([document], { type: "application/pdf" });
  }
  const url = sink.createObjectUrl(blob);
  try {
    sink.save(url, filename);
  } finally {
    sink.revokeObjectUrl(url);
  }
  return { filename };
}
