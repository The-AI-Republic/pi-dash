// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only version snapshot overlay (parity/specs/pages.md §4.5, §6.9,
// PAGE-052/054/079/080). It opens from the `version` key alone, so a
// shared tab-plus-version address reproduces the same snapshot on load.
// The header names the moment the snapshot was saved beside a marker that
// says the body cannot be edited, and while the read is in flight it says
// so instead of guessing a date. The body shows the snapshot's structured
// document when the renderer knows it and the stored markup otherwise. A
// failed read is a panel with a retry that asks for the snapshot again.
// The restore control only exists when the page's editor can take the
// write: the overlay never decides permission itself, it simply renders
// the action it was handed (PAGE-053).

import { Button, IconButton } from "@pidash/kit";
import type { PageVersionDetail } from "@pidash/api-client";
import { X } from "lucide-react";
import * as React from "react";

import { RichTextView } from "../../../shared/editor/RichTextView.js";
import { PageVersionSkeleton } from "./PageDetailLoading.js";
import { snapshotBodyHtml } from "../lib/versionContent.js";

export const PAGE_VERSION_VIEW_ONLY = "View only";
export const PAGE_VERSION_CLOSE = "Close version";
export const PAGE_VERSION_LOADING = "Loading version details";
export const PAGE_VERSION_ERROR = "Something went wrong!";
export const PAGE_VERSION_ERROR_HINT = "The version could not be loaded, please try again.";
export const PAGE_VERSION_RETRY = "Try again";
export const PAGE_VERSION_RESTORE = "Restore";

/** The header's saved-at text, in the reader's locale (year pinned). */
export function snapshotTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

export interface PageVersionOverlayProps {
  versionId: string;
  /** Undefined while the snapshot read is in flight. */
  snapshot: PageVersionDetail | undefined;
  failed: boolean;
  onClose: () => void;
  /** Ask for the snapshot again after a failed read (PAGE-054). */
  onRetry: () => void;
  /**
   * The write the header offers, or null when the reader may not restore
   * (no edit permission) or there is nothing loaded to restore yet.
   */
  restore?: PageVersionRestore | null | undefined;
}

/**
 * The restore action as the shell hands it down; absent without edit
 * rights. It receives the snapshot on screen, so the write is always the
 * body the reader is looking at rather than a second read of the address.
 */
export interface PageVersionRestore {
  onRestore: (snapshot: PageVersionDetail) => void;
}

interface OverlayHeaderProps {
  title: string;
  onClose: () => void;
  /** Null when the snapshot is not known yet or the reader may not write. */
  restore: PageVersionRestore | null;
  /** The snapshot on screen, which the restore writes back. */
  snapshot: PageVersionDetail | undefined;
}

function OverlayHeader({ title, onClose, restore, snapshot }: OverlayHeaderProps): React.ReactElement {
  return (
    <div className="flex shrink-0 items-center justify-between gap-(--space-4) border-b border-(--border) px-(--space-6) py-(--space-3)">
      <div className="flex min-w-0 items-center gap-(--space-3)">
        <h6 className="text-h6 min-w-0 truncate text-(--text)">{title}</h6>
        <span className="text-caption shrink-0 rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) text-(--text-muted)">
          {PAGE_VERSION_VIEW_ONLY}
        </span>
      </div>
      <div className="flex shrink-0 items-center gap-(--space-2)">
        {restore === null || snapshot === undefined ? null : (
          <Button variant="secondary" size="small" onClick={() => restore.onRestore(snapshot)}>
            {PAGE_VERSION_RESTORE}
          </Button>
        )}
        <IconButton label={PAGE_VERSION_CLOSE} size="small" onClick={onClose}>
          <X size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
    </div>
  );
}

function FailedPanel({ onRetry }: { onRetry: () => void }): React.ReactElement {
  return (
    <div data-testid="page-version-error" className="grid flex-1 place-items-center">
      <div className="flex max-w-md flex-col items-center gap-(--space-4) text-center">
        <div>
          <h2 className="text-h4-medium text-(--text)">{PAGE_VERSION_ERROR}</h2>
          <p className="text-body mt-(--space-1) text-(--text-muted)">{PAGE_VERSION_ERROR_HINT}</p>
        </div>
        <Button variant="secondary" size="small" onClick={onRetry}>
          {PAGE_VERSION_RETRY}
        </Button>
      </div>
    </div>
  );
}

export function PageVersionOverlay({
  versionId,
  snapshot,
  failed,
  onClose,
  onRetry,
  restore = null,
}: PageVersionOverlayProps): React.ReactElement {
  // A failed read has no snapshot to write back, and the error panel is
  // the only action on screen (PAGE-054).
  const shownRestore =
    failed || snapshot === undefined || restore === null ? null : { onRestore: () => restore.onRestore(snapshot) };
  return (
    // The overlay covers the canvas, not the whole window: the page header
    // keeps its own controls reachable while a snapshot is open (§4.5, "a
    // full-canvas overlay above the editor"). It sits inside the screen's
    // relative column, so the canvas column is its containing block.
    <div
      data-testid="page-version-overlay"
      className="absolute inset-0 z-40 flex flex-col bg-(--bg)"
      role="dialog"
      aria-label="Version snapshot"
    >
      <OverlayHeader
        title={snapshot === undefined ? PAGE_VERSION_LOADING : snapshotTime(snapshot.last_saved_at)}
        onClose={onClose}
        restore={shownRestore}
        snapshot={snapshot}
      />
      <div
        data-version-id={versionId}
        className="flex min-h-0 flex-1 flex-col overflow-y-auto px-(--space-6) py-(--space-6)"
      >
        {failed ? (
          <FailedPanel onRetry={onRetry} />
        ) : snapshot === undefined ? (
          <PageVersionSkeleton />
        ) : (
          <RichTextView html={snapshotBodyHtml(snapshot)} label="Version body" />
        )}
      </div>
    </div>
  );
}
