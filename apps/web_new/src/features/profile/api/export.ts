// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Today's-activity export (parity/specs/profile.md §3): posts the local
// calendar day, receives the server's CSV bytes and hands them to the
// browser as a download named for the export. The date is computed by the
// caller in the viewer's own calendar so a member near midnight exports
// their today, not UTC's.

import { exportUserProfileActivity, type ApiClient, type ActivityExport } from "@pidash/api-client";

/** Server failures surface as a message the button can toast. */
export const PROFILE_EXPORT_FAILED_TITLE = "Could not download activity";

/** Local calendar date in the payload shape the export expects. */
export function localIsoDate(date: Date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/** Download name for the export: one file per click, never overwritten. */
export function activityExportFilename(atMs: number): string {
  return `profile-activity-${atMs}.csv`;
}

/** The download surface the export needs; injected for tests. */
export interface ExportSink {
  createObjectUrl: (blob: Blob) => string;
  revokeObjectUrl: (url: string) => void;
  save: (url: string, filename: string) => void;
  now: () => number;
}

function browserSink(): ExportSink {
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
    now: () => Date.now(),
  };
}

/** Fetch and save the CSV; resolves with the export the server answered. */
export async function downloadProfileActivity(
  client: ApiClient,
  workspaceSlug: string,
  userId: string,
  sink: ExportSink = browserSink()
): Promise<ActivityExport> {
  const exported = await exportUserProfileActivity(client, workspaceSlug, userId, localIsoDate());
  const blob = new Blob([exported.csv], { type: exported.contentType === "" ? "text/csv" : exported.contentType });
  const url = sink.createObjectUrl(blob);
  try {
    sink.save(url, activityExportFilename(sink.now()));
  } finally {
    sink.revokeObjectUrl(url);
  }
  return exported;
}
