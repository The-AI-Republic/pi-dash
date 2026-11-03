// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One-time token backup download (WSET-027). The create answer's raw secret
// is written into an unnamed Blob and offered as a CSV download; no network
// request and no server copy participate. Exported with injectable DOM hooks
// so the unit tests can observe the anchor without a browser download.

export interface DownloadHooks {
  createObjectURL: (blob: Blob) => string;
  revokeObjectURL: (url: string) => void;
  document: Document;
}

function defaultHooks(): DownloadHooks {
  return {
    createObjectURL: (blob) => URL.createObjectURL(blob),
    revokeObjectURL: (url) => URL.revokeObjectURL(url),
    document,
  };
}

/** Offer `content` as `fileName`; the browser's download event follows. */
export function downloadTokenBackup(fileName: string, content: string, hooks: DownloadHooks = defaultHooks()): void {
  const blob = new Blob([content], { type: "text/csv;charset=utf-8" });
  const url = hooks.createObjectURL(blob);
  const anchor = hooks.document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.rel = "noopener";
  hooks.document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  hooks.revokeObjectURL(url);
}
