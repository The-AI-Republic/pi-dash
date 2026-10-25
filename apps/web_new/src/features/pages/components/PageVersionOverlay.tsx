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
}

function OverlayHeader({ title, onClose }: { title: string; onClose: () => void }): React.ReactElement {
  return (
    <div className="flex shrink-0 items-center justify-between gap-(--space-4) border-b border-(--border) px-(--space-6) py-(--space-3)">
      <div className="flex min-w-0 items-center gap-(--space-3)">
        <h6 className="text-h6 min-w-0 truncate text-(--text)">{title}</h6>
        <span className="text-caption shrink-0 rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) text-(--text-muted)">
          {PAGE_VERSION_VIEW_ONLY}
        </span>
      </div>
      <IconButton label={PAGE_VERSION_CLOSE} size="small" onClick={onClose}>
        <X size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
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
}: PageVersionOverlayProps): React.ReactElement {
  return (
    <div
      data-testid="page-version-overlay"
      className="fixed inset-0 z-40 flex flex-col bg-(--bg)"
      role="dialog"
      aria-modal="true"
      aria-label="Version snapshot"
    >
      <OverlayHeader
        title={snapshot === undefined ? PAGE_VERSION_LOADING : snapshotTime(snapshot.last_saved_at)}
        onClose={onClose}
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
