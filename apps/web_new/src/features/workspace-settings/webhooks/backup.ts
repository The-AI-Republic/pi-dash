// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One-time secret backup (parity/specs/workspace-settings.md §20/§22,
// WSET-021/023). Creating a webhook and rotating its secret both hand the
// person a file with the key, because the key is only revealed once on
// screen. The file is a CSV so it opens in a spreadsheet; the column
// names are ours. Regeneration writes the new key every time — the
// intended behavior behind NEWFRONT-419 — so the builder takes the key
// as an argument instead of reading it from a render closure.

import type { CreatedWorkspaceWebhook } from "@pidash/api-client";

export const BACKUP_COLUMNS = [
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
] as const;

export interface WebhookBackup {
  fileName: string;
  content: string;
}

/** `webhook-secret-key-<milliseconds>.csv` (§20 pins the shape). */
export function backupFileName(now: number): string {
  return `webhook-secret-key-${now}.csv`;
}

/** Quote a cell only when it carries a comma, quote or line break. */
function csvCell(value: string | boolean | number): string {
  const text = typeof value === "boolean" ? String(value) : String(value);
  if (!/[",\r\n]/.test(text)) return text;
  return `"${text.replaceAll('"', '""')}"`;
}

export function webhookBackupCsv(issued: CreatedWorkspaceWebhook, workspaceSlug: string, secretKey: string): string {
  const values: Record<(typeof BACKUP_COLUMNS)[number], string | boolean> = {
    id: issued.id,
    url: issued.url,
    created_at: issued.created_at,
    updated_at: issued.updated_at,
    is_active: issued.is_active,
    project: issued.project,
    cycle: issued.cycle,
    module: issued.module,
    issue: issued.issue,
    issue_comment: issued.issue_comment,
    workspace: workspaceSlug,
    secret_key: secretKey,
  };
  const header = BACKUP_COLUMNS.map(csvCell).join(",");
  const row = BACKUP_COLUMNS.map((column) => csvCell(values[column])).join(",");
  return `${header}\r\n${row}\r\n`;
}

export function webhookBackup(
  issued: CreatedWorkspaceWebhook,
  workspaceSlug: string,
  secretKey: string,
  now: number
): WebhookBackup {
  return { fileName: backupFileName(now), content: webhookBackupCsv(issued, workspaceSlug, secretKey) };
}

/** The subset of the browser globals the download touches, for tests. */
export interface DownloadEnvironment {
  document: Pick<Document, "createElement" | "body">;
  createObjectUrl: (blob: Blob) => string;
  revokeObjectUrl: (url: string) => void;
}

/**
 * Hand a file to the browser through an object URL (no file input, §39).
 * The anchor is attached before it is clicked: detached anchors are
 * ignored by some engines.
 */
export function downloadTextFile(file: WebhookBackup, env: DownloadEnvironment): void {
  const anchor = env.document.createElement("a");
  const url = env.createObjectUrl(new Blob([file.content], { type: "text/csv" }));
  try {
    anchor.href = url;
    anchor.download = file.fileName;
    anchor.rel = "noopener";
    anchor.style.display = "none";
    env.document.body.append(anchor);
    anchor.click();
  } finally {
    anchor.remove();
    env.revokeObjectUrl(url);
  }
}

/** Download through the page's own globals, keeping the key out of the DOM. */
export function downloadWebhookBackup(file: WebhookBackup): void {
  downloadTextFile(file, {
    document: globalThis.document,
    createObjectUrl: (blob) => URL.createObjectURL(blob),
    revokeObjectUrl: (url) => {
      URL.revokeObjectURL(url);
    },
  });
}
