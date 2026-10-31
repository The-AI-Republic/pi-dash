// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Explanatory panel for a forbidden, missing or deleted page
// (parity/specs/pages.md §4.2, PAGE-023). One panel serves all three:
// the viewer never learns which refusal they hit, and the only way out
// is the collection.

import { Link } from "@tanstack/react-router";
import { FileQuestion } from "lucide-react";
import * as React from "react";

import { pageListHref } from "../lib/pageList.js";

export const PAGE_NOT_FOUND_TITLE = "Page not found";
export const PAGE_NOT_FOUND_BODY =
  "This page may be private, or it may no longer exist. You don't have permission to open it.";
export const PAGE_NOT_FOUND_BACK = "View other Pages";

export interface PageDetailPanelProps {
  workspaceSlug: string;
  projectId: string;
}

export function PageDetailPanel({ workspaceSlug, projectId }: PageDetailPanelProps): React.ReactElement {
  return (
    <section
      aria-label={PAGE_NOT_FOUND_TITLE}
      data-testid="page-detail-panel"
      className="mx-auto flex max-w-md flex-col items-center gap-(--space-4) px-(--space-6) py-(--space-16) text-center"
    >
      <FileQuestion size={40} strokeWidth={1.25} aria-hidden="true" className="text-(--text-muted)" />
      <h2 className="text-h4-medium text-(--text)">{PAGE_NOT_FOUND_TITLE}</h2>
      <p className="text-body text-(--text-muted)">{PAGE_NOT_FOUND_BODY}</p>
      <Link
        to={pageListHref(workspaceSlug, projectId)}
        className="text-body rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--accent) hover:underline"
      >
        {PAGE_NOT_FOUND_BACK}
      </Link>
    </section>
  );
}
