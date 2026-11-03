// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  attachmentBaseName,
  attachmentExtension,
  attachmentKind,
  attachmentName,
  attachmentSizeLimit,
  exceedsSizeLimit,
  FALLBACK_FILE_SIZE_LIMIT,
  formatAttachmentSize,
  formatSizeLimit,
  MULTI_FILE_NOTICE,
  oneFileOf,
  oversizeNotice,
} from "./format.js";

function fileOf(size: number): File {
  return new File([new Uint8Array(size)], "a.png", { type: "image/png" });
}

describe("attachmentSizeLimit", () => {
  it("prefers a positive configured ceiling", () => {
    expect(attachmentSizeLimit(1024)).toBe(1024);
  });

  it("falls back when the config omits or invalidates the ceiling", () => {
    expect(attachmentSizeLimit(undefined)).toBe(FALLBACK_FILE_SIZE_LIMIT);
    expect(attachmentSizeLimit(null)).toBe(FALLBACK_FILE_SIZE_LIMIT);
    expect(attachmentSizeLimit(0)).toBe(FALLBACK_FILE_SIZE_LIMIT);
    expect(attachmentSizeLimit(-1)).toBe(FALLBACK_FILE_SIZE_LIMIT);
    expect(attachmentSizeLimit(Number.NaN)).toBe(FALLBACK_FILE_SIZE_LIMIT);
  });
});

describe("sizes", () => {
  it("renders kilobytes under a mebibyte and megabytes above it", () => {
    expect(formatAttachmentSize(0)).toBe("0 KB");
    expect(formatAttachmentSize(-5)).toBe("0 KB");
    expect(formatAttachmentSize(Number.NaN)).toBe("0 KB");
    expect(formatAttachmentSize(1024)).toBe("1 KB");
    expect(formatAttachmentSize(1024 * 1024 - 1)).toBe("1024 KB");
    expect(formatAttachmentSize(1024 * 1024)).toBe("1 MB");
    expect(formatAttachmentSize(5.5 * 1024 * 1024)).toBe("6 MB");
  });

  it("spells the ceiling the way the rejection notice does", () => {
    expect(formatSizeLimit(5 * 1024 * 1024)).toBe("5MB");
    expect(oversizeNotice(5 * 1024 * 1024)).toContain("5MB");
  });

  it("rejects only files above the ceiling", () => {
    expect(exceedsSizeLimit(10, 10)).toBe(false);
    expect(exceedsSizeLimit(11, 10)).toBe(true);
  });
});

describe("names and kinds", () => {
  it("reads the stored name and falls back when absent or blank", () => {
    expect(attachmentName({ attributes: { name: "plan.pdf" } } as never)).toBe("plan.pdf");
    expect(attachmentName({ attributes: { name: "  " } } as never)).toBe("Attachment");
    expect(attachmentName({ attributes: {} } as never)).toBe("Attachment");
    expect(attachmentName({ attributes: { name: 3 } } as never)).toBe("Attachment");
  });

  it("drops only a real extension for the confirmation name", () => {
    expect(attachmentBaseName("oracle-del-1.png")).toBe("oracle-del-1");
    expect(attachmentBaseName("archive.tar.gz")).toBe("archive.tar");
    expect(attachmentBaseName("no-extension")).toBe("no-extension");
    expect(attachmentBaseName(".hidden")).toBe(".hidden");
  });

  it("normalizes extensions and tolerates missing ones", () => {
    expect(attachmentExtension("Photo.PNG")).toBe("png");
    expect(attachmentExtension("trailing.")).toBe("");
    expect(attachmentExtension("none")).toBe("");
  });

  it("maps extension families onto the row icons", () => {
    expect(attachmentKind("a.png")).toBe("image");
    expect(attachmentKind("a.PDF")).toBe("pdf");
    expect(attachmentKind("a.docx")).toBe("document");
    expect(attachmentKind("a.csv")).toBe("document");
    expect(attachmentKind("a.xlsx")).toBe("sheet");
    expect(attachmentKind("a.zip")).toBe("archive");
    expect(attachmentKind("a.bin")).toBe("file");
  });
});

describe("file selection", () => {
  it("accepts exactly one file and never a batch", () => {
    const one = fileOf(1);
    expect(oneFileOf([one])).toBe(one);
    expect(oneFileOf([one, fileOf(1)])).toBeNull();
    expect(oneFileOf([])).toBeNull();
  });

  it("pins the multi-file notice copy", () => {
    expect(MULTI_FILE_NOTICE).toContain("Only one file");
  });
});
