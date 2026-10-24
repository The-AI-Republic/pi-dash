// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared CSV document builder (parity/specs/analytics.md §15,
// ANL-016). Analytics authors this module; NEWFRONT-769 adopts it for
// the remaining export call sites. It owns the byte/quoting rules so
// no feature keeps a private serializer: UTF-8 BOM, CRLF records,
// quoted text where the value needs it, bare finite numbers, and a
// dash for values that cannot be read. An empty row set is an error
// before any Blob or download exists.

/** One output column: its display label and how to read a row value. */
export interface CsvColumn<Row> {
  label: string;
  value: (row: Row) => unknown;
}

/** Let an analytics column model feed the builder without a second type. */
interface CsvColumnLike<Row> {
  label: string;
  exportValue: (row: Row) => unknown;
}

/** A ready-to-download file: name plus document text. */
export interface CsvFile {
  fileName: string;
  content: string;
}

/** Raised when an export has no rows; callers must not download. */
export class CsvEmptyRowsError extends Error {
  constructor() {
    super("CSV export needs at least one row");
    this.name = "CsvEmptyRowsError";
  }
}

const BOM = "\uFEFF";
const CRLF = "\r\n";

/** The single analytics export name (parity/specs/analytics.md §15). */
export function analyticsCsvFileName(workspaceSlug: string): string {
  return `${workspaceSlug}-analytics.csv`;
}

/**
 * One scalar cell. Numbers stay bare; strings are quoted only when they
 * contain a comma, quote, line break or edge whitespace. Null, undefined,
 * NaN and non-scalar values are unreadable and read as a dash.
 */
export function csvCell(value: unknown): string {
  if (value === null || value === undefined) return "-";
  if (typeof value === "number") return Number.isFinite(value) ? String(value) : "-";
  if (typeof value === "boolean") return String(value);
  if (typeof value !== "string") return "-";
  const text = value;
  if (text === "") return "";
  if (/[",\r\n]/.test(text) || text.trim() !== text) {
    return `"${text.replaceAll('"', '""')}"`;
  }
  return text;
}

/**
 * A complete CSV document for the given columns and rows. Headers and
 * records use the same column order. Throws before producing content when
 * there are no rows, so no caller can emit a headers-only file.
 */
export function buildCsvDocument<Row>(columns: readonly CsvColumn<Row>[], rows: readonly Row[]): string {
  if (rows.length === 0) throw new CsvEmptyRowsError();
  const header = columns.map((column) => csvCell(column.label)).join(",");
  const records = rows.map((row) => columns.map((column) => csvCell(column.value(row))).join(","));
  return `${BOM}${header}${CRLF}${records.join(CRLF)}${CRLF}`;
}

/** One analytics export: workspace-derived name and the CSV document. */
export function buildAnalyticsCsv<Row>(
  workspaceSlug: string,
  columns: readonly CsvColumn<Row>[],
  rows: readonly Row[]
): CsvFile {
  return { fileName: analyticsCsvFileName(workspaceSlug), content: buildCsvDocument(columns, rows) };
}

/** Analytics columns already carry display labels and export readers. */
export function analyticsCsvColumns<Row>(columns: readonly CsvColumnLike<Row>[]): CsvColumn<Row>[] {
  return columns.map((column) => ({ label: column.label, value: column.exportValue }));
}

/** Browser hooks the download touches, injectable for unit tests. */
export interface CsvDownloadEnvironment {
  document: Pick<Document, "createElement" | "body">;
  createObjectUrl: (blob: Blob) => string;
  revokeObjectUrl: (url: string) => void;
}

function browserEnvironment(): CsvDownloadEnvironment {
  return {
    document,
    createObjectUrl: (blob) => URL.createObjectURL(blob),
    revokeObjectUrl: (url) => {
      URL.revokeObjectURL(url);
    },
  };
}

/**
 * Offer a built file through an object URL and a temporary anchor. The
 * anchor is attached before activation because some browsers ignore a
 * detached one.
 */
export function downloadCsvFile(file: CsvFile, environment: CsvDownloadEnvironment = browserEnvironment()): void {
  const anchor = environment.document.createElement("a");
  const url = environment.createObjectUrl(new Blob([file.content], { type: "text/csv;charset=utf-8" }));
  try {
    anchor.href = url;
    anchor.download = file.fileName;
    anchor.rel = "noopener";
    anchor.style.display = "none";
    environment.document.body.append(anchor);
    anchor.click();
  } finally {
    anchor.remove();
    environment.revokeObjectUrl(url);
  }
}
