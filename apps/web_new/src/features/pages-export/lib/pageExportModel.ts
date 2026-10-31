// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export dialog model (parity/specs/exports.md §5, EXP-016–021):
// the format/scope/size choices, their defaults, and the title-slug and
// file-name rules the renderers share. The size choice is preserved while
// hidden (Markdown) and restored when the document format returns.
export type PageExportFormat = "PDF" | "Markdown";

export type PageExportScope = "Everything" | "No images";

export type PageExportSize = "A4" | "A3" | "A2" | "Letter" | "Legal" | "Tabloid";

/** Format options in dialog order. */
export const PAGE_EXPORT_FORMATS: readonly PageExportFormat[] = ["PDF", "Markdown"];

/** Content-scope options in dialog order. */
export const PAGE_EXPORT_SCOPES: readonly PageExportScope[] = ["Everything", "No images"];

/** Page-size options in dialog order. */
export const PAGE_EXPORT_SIZES: readonly PageExportSize[] = ["A4", "A3", "A2", "Letter", "Legal", "Tabloid"];

export const DEFAULT_PAGE_EXPORT_FORMAT: PageExportFormat = "PDF";

export const DEFAULT_PAGE_EXPORT_SCOPE: PageExportScope = "Everything";

export const DEFAULT_PAGE_EXPORT_SIZE: PageExportSize = "A4";

/** The dialog's choices. `size` keeps its value while hidden. */
export interface PageExportForm {
  format: PageExportFormat;
  scope: PageExportScope;
  size: PageExportSize;
}

/** Fresh defaults: PDF / Everything / A4. */
export function defaultPageExportForm(): PageExportForm {
  return {
    format: DEFAULT_PAGE_EXPORT_FORMAT,
    scope: DEFAULT_PAGE_EXPORT_SCOPE,
    size: DEFAULT_PAGE_EXPORT_SIZE,
  };
}

/** The size row renders only for the document format. */
export function isPageExportSizeVisible(format: PageExportFormat): boolean {
  return format === "PDF";
}

/**
 * File-name slug for a page title (oracle parity: lowercase, every run of
 * characters outside [a-z0-9-_] becomes one dash; leading/trailing dashes
 * are kept, matching the oracle's helper exactly).
 */
export function pageExportTitleSlug(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9-_]/g, "-")
    .replace(/-+/g, "-");
}

/** `{slug}-{size}.pdf`, with the size lowercased. */
export function pageExportPdfFilename(title: string, size: PageExportSize): string {
  return `${pageExportTitleSlug(title)}-${size.toLowerCase()}.pdf`;
}

/** `{slug}.md`. */
export function pageExportMarkdownFilename(title: string): string {
  return `${pageExportTitleSlug(title)}.md`;
}
