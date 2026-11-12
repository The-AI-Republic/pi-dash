// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail loading surfaces (parity/specs/pages.md §2.7, PAGE-079). The
// record read gets a centered brand spinner; the body and a version
// snapshot get skeleton blocks, announced as one status region so a
// screen reader hears the wait once instead of per block.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

/** Centered brand mark while a record is unresolved. */
export function PageDetailSpinner(): React.ReactElement {
  return (
    <div data-testid="page-detail-spinner" className="grid size-full place-items-center" aria-label="Loading page">
      <svg
        viewBox="0 0 64 64"
        role="img"
        aria-hidden="true"
        className="size-16 animate-pulse"
        xmlns="http://www.w3.org/2000/svg"
      >
        <rect x="4" y="4" width="56" height="56" rx="14" fill="var(--text)" opacity="0.08" />
        <rect x="15" y="27" width="34" height="6" rx="3" fill="var(--text-muted)" />
        <rect x="15" y="38" width="22" height="6" rx="3" fill="var(--accent)" />
        <rect x="15" y="17" width="12" height="6" rx="3" fill="var(--accent)" />
      </svg>
    </div>
  );
}

/** Body placeholder: paragraph lines at the canvas width. */
export function PageBodySkeleton(): React.ReactElement {
  return (
    <div
      role="status"
      aria-label="Loading page body"
      data-testid="page-body-skeleton"
      className="flex flex-col gap-(--space-4)"
    >
      <Skeleton height="14px" width="88%" />
      <Skeleton height="14px" width="72%" />
      <Skeleton height="14px" width="80%" />
      <Skeleton height="14px" width="54%" />
    </div>
  );
}

/** Snapshot placeholder inside the version overlay. */
export function PageVersionSkeleton(): React.ReactElement {
  return (
    <div
      role="status"
      aria-label="Loading version"
      data-testid="page-version-skeleton"
      className="flex flex-col gap-(--space-4)"
    >
      <Skeleton height="14px" width="90%" />
      <Skeleton height="14px" width="64%" />
      <Skeleton height="14px" width="76%" />
    </div>
  );
}
