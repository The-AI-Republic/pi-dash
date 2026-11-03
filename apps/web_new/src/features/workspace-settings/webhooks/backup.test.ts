// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Secret backup file (parity/specs/workspace-settings.md §20/§22,
// WSET-021/023): the pinned file name, the CSV columns, quoting, and the
// download hand-off through an object URL.
import { describe, expect, it, vi } from "vitest";

import type { CreatedWorkspaceWebhook } from "@pidash/api-client";
import {
  BACKUP_COLUMNS,
  backupFileName,
  downloadWebhookBackup,
  downloadTextFile,
  webhookBackup,
  webhookBackupCsv,
} from "./backup.js";

function issued(overrides: Partial<CreatedWorkspaceWebhook> = {}): CreatedWorkspaceWebhook {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    url: "https://example.com/post",
    is_active: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    project: true,
    issue: true,
    cycle: true,
    module: true,
    issue_comment: true,
    secret_key: "whsec_fixture_1234567890",
    ...overrides,
  };
}

describe("webhook secret backup", () => {
  it("names the file with the timestamp", () => {
    expect(backupFileName(1_760_000_000_000)).toBe("webhook-secret-key-1760000000000.csv");
  });

  it("writes every column in the pinned order", () => {
    expect(BACKUP_COLUMNS).toEqual([
      "id",
      "url",
      "created_at",
      "updated_at",
      "is_active",
      "project",
      "cycle",
      "module",
      "issue",
      "issue_comment",
      "workspace",
      "secret_key",
    ]);
  });

  it("carries the address, flags, workspace and the given key", () => {
    const content = webhookBackupCsv(issued(), "acme", "whsec_fixture_1234567890");
    const [header, row] = content.trim().split("\r\n");
    expect(header).toBe(BACKUP_COLUMNS.join(","));
    expect(row).toBe(
      "223e4567-e89b-12d3-a456-426614174001,https://example.com/post,2026-01-01T00:00:00Z,2026-01-02T00:00:00Z,true,true,true,true,true,true,acme,whsec_fixture_1234567890"
    );
  });

  it("quotes cells carrying a comma, a quote or a line break", () => {
    const content = webhookBackupCsv(issued({ url: 'https://example.com/a,b"c' }), "acme", 'k"ey');
    expect(content).toContain('"https://example.com/a,b""c"');
    expect(content).toContain('"k""ey"');
    const multiline = webhookBackupCsv(issued({ url: "https://example.com/a\nb" }), "acme", "k");
    expect(multiline).toContain('"https://example.com/a\nb"');
  });

  it("builds the file with the fresh key it is given, not a stored one", () => {
    const fresh = webhookBackup(issued(), "acme", "brand-new-key", 42);
    expect(fresh.fileName).toBe("webhook-secret-key-42.csv");
    expect(fresh.content).toContain("brand-new-key");
  });

  it("attaches, clicks and cleans up the download anchor", () => {
    const click = vi.fn();
    const remove = vi.fn();
    const append = vi.fn();
    const anchor = {
      href: "",
      download: "",
      rel: "",
      style: { display: "" },
      click,
      remove,
    } as unknown as HTMLAnchorElement;
    const createObjectUrl = vi.fn(() => "blob:one");
    const revokeObjectUrl = vi.fn();
    downloadTextFile(
      { fileName: "x.csv", content: "a,b" },
      {
        document: { createElement: (() => anchor) as Document["createElement"], body: { append } as never },
        createObjectUrl,
        revokeObjectUrl,
      }
    );
    expect(createObjectUrl).toHaveBeenCalledWith(expect.any(Blob));
    expect(anchor.href).toBe("blob:one");
    expect(anchor.download).toBe("x.csv");
    expect(append).toHaveBeenCalledWith(anchor);
    expect(click).toHaveBeenCalledTimes(1);
    expect(remove).toHaveBeenCalledTimes(1);
    expect(revokeObjectUrl).toHaveBeenCalledWith("blob:one");
  });

  it("downloads through the page globals, creating and releasing one object URL", () => {
    const createObjectURL = vi.fn(() => "blob:page");
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, "createObjectURL", { configurable: true, value: createObjectURL });
    Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revokeObjectURL });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    try {
      downloadWebhookBackup({ fileName: "webhook-secret-key-7.csv", content: "id,secret_key\r\n1,k\r\n" });
      expect(createObjectURL).toHaveBeenCalledWith(expect.any(Blob));
      expect(click).toHaveBeenCalledTimes(1);
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:page");
      expect(document.querySelector("a")).toBeNull();
    } finally {
      click.mockRestore();
    }
  });
});
