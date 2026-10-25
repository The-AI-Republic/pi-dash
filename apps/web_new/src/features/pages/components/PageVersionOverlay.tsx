// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only version snapshot overlay (parity/specs/pages.md §4.5, §6.9,
// PAGE-052/079/080; the timeline and restore actions belong to
// NEWFRONT-844). Opened from the `version` key alone, so a shared
// tab-plus-version address reproduces the same snapshot on load. The
// header carries the read-only marker; the body renders the snapshot's
// stored markup, or skeleton blocks while its read is in flight.

import { IconButton } from "@pidash/kit";
import type { PageVersionDetail } from "@pidash/api-client";
import { X } from "lucide-react";
import * as React from "react";

import { RichTextView } from "../../../shared/editor/RichTextView.js";
import { PageVersionSkeleton } from "./PageDetailLoading.js";

export const PAGE_VERSION_VIEW_ONLY = "View only";
export const PAGE_VERSION_CLOSE = "Close version";
export const PAGE_VERSION_ERROR = "Something went wrong!";
export const PAGE_VERSION_ERROR_HINT = "This version could not be loaded. Close the overlay and try again.";

export interface PageVersionOverlayProps {
  versionId: string;
  /** Undefined while the snapshot read is in flight. */
  snapshot: PageVersionDetail | undefined;
  failed: boolean;
  onClose: () => void;
}

function snapshotTime(value: string | undefined): string {
  if (value === undefined) return "Version snapshot";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Version snapshot";
  return `Version from ${date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  })}`;
}

export function PageVersionOverlay({
  versionId,
  snapshot,
  failed,
  onClose,
}: PageVersionOverlayProps): React.ReactElement {
  return (
    <div
      data-testid="page-version-overlay"
      className="fixed inset-0 z-40 flex flex-col bg-(--bg)"
      role="dialog"
      aria-modal="true"
      aria-label="Version snapshot"
    >
      <div className="flex shrink-0 items-center justify-between border-b border-(--border) px-(--space-6) py-(--space-3)">
        <div className="flex min-w-0 items-center gap-(--space-3)">
          <h6 className="text-h6 truncate text-(--text)">{snapshotTime(snapshot?.last_saved_at)}</h6>
          <span className="text-caption rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) text-(--text-muted)">
            {PAGE_VERSION_VIEW_ONLY}
          </span>
        </div>
        <IconButton label={PAGE_VERSION_CLOSE} size="small" onClick={onClose}>
          <X size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
      <div data-version-id={versionId} className="min-h-0 flex-1 overflow-y-auto px-(--space-6) py-(--space-6)">
        {failed ? (
          <div className="flex flex-col gap-(--space-2)">
            <h2 className="text-h4-medium text-(--text)">{PAGE_VERSION_ERROR}</h2>
            <p className="text-body text-(--text-muted)">{PAGE_VERSION_ERROR_HINT}</p>
          </div>
        ) : snapshot === undefined ? (
          <PageVersionSkeleton />
        ) : (
          <RichTextView html={snapshot.description_html} label="Version body" />
        )}
      </div>
    </div>
  );
}
