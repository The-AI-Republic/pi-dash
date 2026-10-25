// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Within-project move picker (parity/specs/pages.md §12.2, PAGE-087). The
// page owner or a project admin picks a container from the pages the caller
// can see, or the top level, and confirms; the page and its whole subtree
// follow. The picker never offers the page itself, any of its descendants,
// or an archived page, and says why a choice is refused if one slips
// through. This is a different feature from the cross-project move
// (PAGE-017) and never shares that dialog.

import { Button, Dialog } from "@pidash/kit";
import type { PageRow as PageRowData } from "@pidash/api-client";
import * as React from "react";

import { pageDisplayTitle } from "../lib/pageList.js";
import {
  filterMoveTargets,
  pageMoveRefusal,
  pageMoveRefusalMessage,
  pageMoveTargets,
  pageParentPath,
  PAGE_MOVE_DIALOG_TITLE,
  PAGE_MOVE_TOP_LEVEL,
  PAGE_MOVE_TOP_LEVEL_HINT,
} from "../lib/pageNested.js";

/** Input prompt of the picker's search field. */
export const PAGE_MOVE_SEARCH_PLACEHOLDER = "Search pages";
/** Plain line when nothing matches the query or nothing can receive the page. */
export const PAGE_MOVE_EMPTY = "No pages found";

export interface PageNestedMoveDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The page being moved. */
  page: Pick<PageRowData, "id" | "name" | "parent" | "parent_id">;
  /** Every page the caller has loaded; supplies targets and paths. */
  pool: readonly PageRowData[];
  pending: boolean;
  /** The full target pool is still loading; the picker is not ready yet. */
  loading?: boolean;
  /** Runs with the chosen container id, or null for the top level. */
  onConfirm: (targetId: string | null) => void;
}

export function PageNestedMoveDialog({
  open,
  onOpenChange,
  page,
  pool,
  pending,
  loading = false,
  onConfirm,
}: PageNestedMoveDialogProps): React.ReactElement {
  const currentParent = page.parent_id ?? page.parent;
  const [target, setTarget] = React.useState<string | null>(currentParent);
  const [query, setQuery] = React.useState("");

  React.useEffect(() => {
    if (!open) return;
    setTarget(currentParent);
    setQuery("");
  }, [open, currentParent]);

  const candidates = React.useMemo(() => {
    const allowed = pageMoveTargets(pool, page.id);
    return filterMoveTargets(allowed, pool, query);
  }, [pool, page.id, query]);

  const refusal = pageMoveRefusal(pool, page.id, target);
  const refusalMessage = refusal === null ? null : pageMoveRefusalMessage(refusal);
  const title = pageDisplayTitle(page.name);

  return (
    <Dialog
      title={PAGE_MOVE_DIALOG_TITLE}
      description={`Choose a new place for “${title}”. Its sub-pages move with it.`}
      open={open}
      onOpenChange={(next) => {
        if (!next && pending) return;
        onOpenChange(next);
      }}
      actions={
        <>
          <Button variant="secondary" type="button" disabled={pending} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            type="button"
            loading={pending}
            disabled={pending || refusalMessage !== null}
            onClick={() => onConfirm(target)}
          >
            {pending ? "Moving…" : "Move page"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-2)">
        <input
          type="search"
          aria-label={PAGE_MOVE_SEARCH_PLACEHOLDER}
          placeholder={PAGE_MOVE_SEARCH_PLACEHOLDER}
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
          className="text-body h-8 w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text)"
        />
        <ul
          role="listbox"
          aria-label={PAGE_MOVE_DIALOG_TITLE}
          aria-busy={loading || undefined}
          data-testid="page-move-listbox"
          data-ready={loading ? "false" : "true"}
          className="max-h-72 overflow-y-auto"
        >
          <li>
            <button
              type="button"
              role="option"
              aria-selected={target === null}
              onClick={() => setTarget(null)}
              className={`flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left ${
                target === null ? "bg-(--subtle) text-(--text)" : "text-(--text-muted) hover:bg-(--subtle)"
              }`}
            >
              <span className="text-body">{PAGE_MOVE_TOP_LEVEL}</span>
              <span className="text-meta text-(--text-muted)">{PAGE_MOVE_TOP_LEVEL_HINT}</span>
            </button>
          </li>
          {candidates.map((candidate) => {
            const path = pageParentPath(pool, candidate.id);
            return (
              <li key={candidate.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={target === candidate.id}
                  onClick={() => setTarget(candidate.id)}
                  className={`flex w-full flex-col rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left ${
                    target === candidate.id ? "bg-(--subtle) text-(--text)" : "text-(--text-muted) hover:bg-(--subtle)"
                  }`}
                >
                  <span className="text-body truncate">{pageDisplayTitle(candidate.name)}</span>
                  {path === null ? null : <span className="text-meta truncate">{path}</span>}
                </button>
              </li>
            );
          })}
        </ul>
        {!loading && candidates.length === 0 ? (
          <p role="status" className="text-body px-(--space-2) py-(--space-2) text-(--text-muted)">
            {PAGE_MOVE_EMPTY}
          </p>
        ) : null}
        {refusalMessage === null ? null : (
          <p role="alert" data-testid="page-move-refusal" className="text-body text-(--danger)">
            {refusalMessage}
          </p>
        )}
      </div>
    </Dialog>
  );
}
