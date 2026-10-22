// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Byte leg for attachment uploads (parity/specs/issues.md §15, ISS-188).
// The signed POST runs in an XMLHttpRequest so the in-flight row can
// report a live percentage; the contract's default fetch upload cannot
// observe progress.

import type { AttachmentByteUpload, AttachmentByteUploadResponse } from "@pidash/api-client";

/** Live percentage of the bytes leg, 0–100. */
export type UploadProgress = (percent: number) => void;

/**
 * The contract's byte-upload injection, backed by XHR. Resolves with the
 * server's status once the signed POST settles; a transport failure
 * rejects so the mutation reports a failed attach.
 */
export function uploadAttachmentBytes(onProgress: UploadProgress): AttachmentByteUpload {
  return (url, body) =>
    new Promise<AttachmentByteUploadResponse>((resolve, reject) => {
      const request = new XMLHttpRequest();
      request.open("POST", url);
      request.upload.addEventListener("progress", (event) => {
        if (!event.lengthComputable || event.total <= 0) return;
        const percent = Math.round((event.loaded / event.total) * 100);
        onProgress(Math.min(100, Math.max(0, percent)));
      });
      request.addEventListener("load", () => {
        resolve({ ok: request.status >= 200 && request.status < 300, status: request.status });
      });
      request.addEventListener("error", () => {
        reject(new Error("The attachment bytes could not be sent."));
      });
      request.send(body);
    });
}
