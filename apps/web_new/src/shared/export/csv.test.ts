// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared CSV builder tests (parity/specs/analytics.md §15, ANL-016).
import { describe, expect, it, vi } from "vitest";

import {
  analyticsCsvFileName,
  buildAnalyticsCsv,
  buildCsvDocument,
  csvCell,
  CsvEmptyRowsError,
  downloadCsvFile,
  type CsvColumn,
} from "./csv.js";

interface Row {
  name: unknown;
  count: unknown;
}

const COLUMNS: CsvColumn<Row>[] = [
  { label: "Name", value: (row) => row.name },
  { label: "Count", value: (row) => row.count },
];

describe("csvCell", () => {
  it("leaves simple text bare and writes finite numbers bare", () => {
    expect(csvCell("Urgent")).toBe("Urgent");
    expect(csvCell(3)).toBe("3");
    expect(csvCell(0)).toBe("0");
    expect(csvCell(false)).toBe("false");
  });

  it("quotes commas, quotes, line breaks and edge whitespace", () => {
    expect(csvCell("Alpha, Beta")).toBe('"Alpha, Beta"');
    expect(csvCell('say "hi"')).toBe('"say ""hi"""');
    expect(csvCell("line\r\nbreak")).toBe('"line\r\nbreak"');
    expect(csvCell(" padded ")).toBe('" padded "');
  });

  it("writes a dash only for values that cannot be read", () => {
    expect(csvCell(null)).toBe("-");
    expect(csvCell(undefined)).toBe("-");
    expect(csvCell(Number.NaN)).toBe("-");
    expect(csvCell({ nested: true })).toBe("-");
    expect(csvCell("")).toBe("");
  });
});

describe("buildCsvDocument", () => {
  it("writes a BOM, a header row, CRLF records and a trailing CRLF", () => {
    const document = buildCsvDocument(COLUMNS, [
      { name: "Urgent", count: 2 },
      { name: "Alpha, Beta", count: 1 },
    ]);
    expect(document).toBe('\uFEFFName,Count\r\nUrgent,2\r\n"Alpha, Beta",1\r\n');
  });

  it("uses display labels for headers and dash for an unreadable cell", () => {
    const document = buildCsvDocument(COLUMNS, [{ name: null, count: Number.POSITIVE_INFINITY }]);
    expect(document).toBe("\uFEFFName,Count\r\n-,-\r\n");
  });

  it("rejects an empty row set before producing any content", () => {
    expect(() => buildCsvDocument(COLUMNS, [])).toThrow(CsvEmptyRowsError);
  });
});

describe("analytics files", () => {
  it("names the file from the workspace slug", () => {
    expect(analyticsCsvFileName("acme")).toBe("acme-analytics.csv");
    expect(buildAnalyticsCsv("acme", COLUMNS, [{ name: "Urgent", count: 1 }]).fileName).toBe("acme-analytics.csv");
  });

  it("builds the analytics document with the shared rules", () => {
    expect(buildAnalyticsCsv("acme", COLUMNS, [{ name: "Urgent", count: 1 }]).content).toBe(
      "\uFEFFName,Count\r\nUrgent,1\r\n"
    );
  });
});

describe("downloadCsvFile", () => {
  it("offers the content as a CSV blob through an attached anchor", () => {
    const click = vi.fn();
    const remove = vi.fn();
    const anchor = { href: "", download: "", rel: "", style: {}, click, remove } as unknown as HTMLAnchorElement;
    const append = vi.fn();
    const documentStub = {
      createElement: vi.fn(() => anchor),
      body: { append },
    } as unknown as Pick<Document, "createElement" | "body">;
    const revokeObjectUrl = vi.fn();
    const createObjectUrl = vi.fn((_blob: Blob) => "blob:csv");

    downloadCsvFile(
      { fileName: "acme-analytics.csv", content: "body" },
      { document: documentStub, createObjectUrl, revokeObjectUrl }
    );

    expect(createObjectUrl).toHaveBeenCalledOnce();
    const [blob] = createObjectUrl.mock.calls[0] ?? [];
    expect(blob?.type).toBe("text/csv;charset=utf-8");
    expect(append).toHaveBeenCalledWith(anchor);
    expect(anchor.download).toBe("acme-analytics.csv");
    expect(anchor.href).toBe("blob:csv");
    expect(click).toHaveBeenCalledOnce();
    expect(remove).toHaveBeenCalledOnce();
    expect(revokeObjectUrl).toHaveBeenCalledWith("blob:csv");
  });

  it("still removes the anchor and revokes the URL when activation throws", () => {
    const remove = vi.fn();
    const anchor = {
      href: "",
      download: "",
      rel: "",
      style: {},
      click: vi.fn(() => {
        throw new Error("blocked");
      }),
      remove,
    } as unknown as HTMLAnchorElement;
    const revokeObjectUrl = vi.fn();

    expect(() =>
      downloadCsvFile(
        { fileName: "acme-analytics.csv", content: "body" },
        {
          document: {
            createElement: () => anchor,
            body: { append: vi.fn() },
          } as unknown as Pick<Document, "createElement" | "body">,
          createObjectUrl: () => "blob:csv",
          revokeObjectUrl,
        }
      )
    ).toThrow("blocked");
    expect(remove).toHaveBeenCalledOnce();
    expect(revokeObjectUrl).toHaveBeenCalledWith("blob:csv");
  });
});
