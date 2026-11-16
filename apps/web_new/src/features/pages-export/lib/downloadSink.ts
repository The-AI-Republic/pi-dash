// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export download surface (parity/specs/exports.md §5, EXP-019–021):
// the browser primitives the export needs, injected for tests.
export interface PageExportSink {
  createObjectUrl: (blob: Blob) => string;
  revokeObjectUrl: (url: string) => void;
  save: (url: string, filename: string) => void;
}

/** The live browser download surface. */
export function browserPageExportSink(): PageExportSink {
  return {
    createObjectUrl: (blob) => URL.createObjectURL(blob),
    revokeObjectUrl: (url) => {
      URL.revokeObjectURL(url);
    },
    save: (url, filename) => {
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = filename;
      document.body.appendChild(anchor);
      anchor.click();
      document.body.removeChild(anchor);
    },
  };
}
