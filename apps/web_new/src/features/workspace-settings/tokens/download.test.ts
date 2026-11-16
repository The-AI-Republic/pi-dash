// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { downloadTokenBackup, type DownloadHooks } from "./download.js";

afterEach(() => {
  document.body.replaceChildren();
});

describe("downloadTokenBackup", () => {
  it("offers a CSV blob under the file name and revokes the URL", () => {
    const blobs: Blob[] = [];
    const anchors: HTMLAnchorElement[] = [];
    const clicks: string[] = [];
    const hooks: DownloadHooks = {
      createObjectURL: (blob) => {
        blobs.push(blob);
        return "blob:token";
      },
      revokeObjectURL: vi.fn(),
      document: {
        createElement: ((tag: string) => {
          const element = document.createElementNS("http://www.w3.org/1999/xhtml", tag) as HTMLAnchorElement;
          if (tag === "a") {
            const click = element.click.bind(element);
            element.click = () => {
              clicks.push(element.download);
              click();
            };
            anchors.push(element);
          }
          return element;
        }) as Document["createElement"],
        body: document.body,
      } as Document,
    };

    downloadTokenBackup("secret-key-1.csv", "Title,Description\nA,B\n", hooks);

    expect(anchors).toHaveLength(1);
    expect(anchors[0]?.href).toBe("blob:token");
    expect(anchors[0]?.download).toBe("secret-key-1.csv");
    expect(clicks).toEqual(["secret-key-1.csv"]);
    expect(hooks.revokeObjectURL).toHaveBeenCalledWith("blob:token");
    expect(document.body.querySelector("a")).toBeNull();
    expect(blobs[0]?.type).toBe("text/csv;charset=utf-8");
  });

  it("uses the browser's URL factory and document by default", async () => {
    const create = vi.fn((_blob: Blob) => "blob:default");
    const revoke = vi.fn();
    const original = { create: URL.createObjectURL, revoke: URL.revokeObjectURL };
    Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    try {
      downloadTokenBackup("secret-key-2.csv", "body");
      expect(create).toHaveBeenCalledOnce();
      const blob = create.mock.calls[0]?.[0] as Blob;
      expect(blob.type).toBe("text/csv;charset=utf-8");
      expect(revoke).toHaveBeenCalledWith("blob:default");
      expect(document.body.querySelector("a")).toBeNull();
      expect(click).toHaveBeenCalledOnce();
    } finally {
      Object.assign(URL, original);
      click.mockRestore();
    }
  });
});
