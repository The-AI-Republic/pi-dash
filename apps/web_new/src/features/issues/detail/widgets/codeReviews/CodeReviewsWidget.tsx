// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Code-review links (parity/specs/issues.md §16, ISS-205). The section
// mounts only when the issue has at least one linked review, sits above
// the activity area, and carries a badge plus a new-tab title per row.
// Members attach by pasting a PR/MR URL; the form trims it, clears on
// success and keeps the value when the server refuses it. Detaching
// removes the row without a confirmation.

import { Badge, Button, IconButton, Input } from "@pidash/kit";
import type { CodeReviewLink } from "@pidash/api-client";
import { GitPullRequest, X } from "lucide-react";
import * as React from "react";

import { useAttachCodeReview, useCodeReviews, useDetachCodeReview } from "./api.js";
import {
  CODE_REVIEWS_TITLE,
  CODE_REVIEW_URL_PLACEHOLDER,
  reviewBadge,
  reviewTitle,
  type ReviewBadge as ReviewBadgeLabel,
} from "./format.js";

export interface CodeReviewsWidgetProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Attach and detach hide for guests, archived and synced items. */
  editable: boolean;
}

const BADGE_TONES: Record<ReviewBadgeLabel, "success" | "neutral" | "accent" | "warning"> = {
  Merged: "success",
  Draft: "neutral",
  Open: "accent",
  Closed: "warning",
};

function ReviewRow({
  link,
  editable,
  detaching,
  onDetach,
}: {
  link: CodeReviewLink;
  editable: boolean;
  detaching: boolean;
  onDetach: () => void;
}): React.ReactElement {
  const badge = reviewBadge(link);
  return (
    <div
      data-widget-row="Code reviews"
      className="group/row flex items-center gap-(--space-3) rounded-(--radius-control) px-(--space-1) py-(--space-1) hover:bg-(--subtle)"
    >
      <Badge tone={BADGE_TONES[badge]} dot>
        {badge}
      </Badge>
      <a
        href={link.url}
        target="_blank"
        rel="noreferrer noopener"
        className="text-body min-w-0 flex-1 truncate text-(--text) hover:underline"
      >
        {reviewTitle(link)}
      </a>
      {editable ? (
        <span className="shrink-0 opacity-0 group-hover/row:opacity-100 focus-within:opacity-100">
          <IconButton label={`Detach ${reviewTitle(link)}`} size="small" onClick={onDetach} disabled={detaching}>
            <X size={14} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </span>
      ) : null}
    </div>
  );
}

export function CodeReviewsWidget({
  workspaceSlug,
  projectId,
  issueId,
  editable,
}: CodeReviewsWidgetProps): React.ReactElement | null {
  const query = useCodeReviews(workspaceSlug, projectId, issueId);
  const attach = useAttachCodeReview(workspaceSlug, projectId, issueId);
  const detach = useDetachCodeReview(workspaceSlug, projectId, issueId);
  const [url, setUrl] = React.useState("");
  const [open, setOpen] = React.useState(true);
  const rows = query.data ?? [];

  if (query.isPending || rows.length === 0) return null;

  function submit(event: React.FormEvent): void {
    event.preventDefault();
    attach.mutate(url.trim(), {
      onSuccess: () => setUrl(""),
    });
  }

  return (
    <section data-widget-root="Code reviews" aria-label={CODE_REVIEWS_TITLE} className="flex flex-col gap-(--space-2)">
      {/* The header is direct inside the section so the content panel is a sibling. */}
      <button
        type="button"
        data-widget-header="Code reviews"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left font-medium text-(--text) hover:bg-(--subtle)"
      >
        <GitPullRequest size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
        <span className="truncate">{CODE_REVIEWS_TITLE}</span>
        <span data-widget-count className="text-meta text-(--text-muted)">
          {rows.length}
        </span>
      </button>
      {open ? (
        <div className="grid gap-(--space-2) overflow-hidden" data-widget-rows="Code reviews">
          <div className="flex flex-col">
            {rows.map((link) => (
              <ReviewRow
                key={link.id}
                link={link}
                editable={editable}
                detaching={detach.isPending && detach.variables === link.id}
                onDetach={() => detach.mutate(link.id)}
              />
            ))}
          </div>
          {editable ? (
            <form className="flex items-end gap-(--space-2)" onSubmit={submit}>
              <div className="min-w-0 flex-1">
                <Input
                  label="Review link"
                  placeholder={CODE_REVIEW_URL_PLACEHOLDER}
                  value={url}
                  onChange={(event) => setUrl(event.target.value)}
                  className="w-full"
                />
              </div>
              <Button type="submit" variant="secondary" loading={attach.isPending} disabled={url.trim() === ""}>
                Attach
              </Button>
            </form>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
