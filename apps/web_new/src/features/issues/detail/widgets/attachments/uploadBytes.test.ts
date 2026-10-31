// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { uploadAttachmentBytes } from "./uploadBytes.js";

interface Handlers {
  progress: (event: ProgressEvent) => void;
  load: () => void;
  error: () => void;
}

function installXhr(): { handlers: Handlers; status: { value: number } } {
  const handlers = { progress: () => undefined, load: () => undefined, error: () => undefined } as Handlers;
  const status = { value: 0 };
  class FakeXhr {
    upload = {
      addEventListener: (_type: string, listener: (event: ProgressEvent) => void) => {
        handlers.progress = listener;
      },
    };
    get status(): number {
      return status.value;
    }
    addEventListener(type: string, listener: () => void): void {
      if (type === "load") handlers.load = listener;
      if (type === "error") handlers.error = listener;
    }
    open(): void {}
    send(): void {}
  }
  vi.stubGlobal("XMLHttpRequest", FakeXhr as never);
  return { handlers, status };
}

afterEach(() => vi.unstubAllGlobals());

describe("uploadAttachmentBytes", () => {
  it("reports a clamped, rounded percentage while the bytes travel", async () => {
    const { handlers, status } = installXhr();
    const seen: number[] = [];
    const upload = uploadAttachmentBytes((percent) => seen.push(percent));
    const pending = upload("https://storage.test/upload", new FormData());
    handlers.progress({ lengthComputable: true, loaded: 41, total: 100 } as ProgressEvent);
    handlers.progress({ lengthComputable: true, loaded: 500, total: 100 } as ProgressEvent);
    handlers.progress({ lengthComputable: false, loaded: 10, total: 0 } as ProgressEvent);
    status.value = 204;
    handlers.load();
    await expect(pending).resolves.toEqual({ ok: true, status: 204 });
    expect(seen).toEqual([41, 100]);
  });

  it("resolves ok false for a refused byte leg", async () => {
    const { handlers, status } = installXhr();
    const pending = uploadAttachmentBytes(() => undefined)("https://storage.test/upload", new FormData());
    status.value = 403;
    handlers.load();
    await expect(pending).resolves.toEqual({ ok: false, status: 403 });
  });

  it("rejects when the request itself fails", async () => {
    const { handlers } = installXhr();
    const pending = uploadAttachmentBytes(() => undefined)("https://storage.test/upload", new FormData());
    handlers.error();
    await expect(pending).rejects.toThrow("The attachment bytes could not be sent.");
  });
});
