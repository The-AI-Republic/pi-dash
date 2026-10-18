// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One list row (parity/specs/issues.md §2, ISS-013). The row is a real
// anchor to the browse route; a plain click opens peek instead of
// navigating, while modified/middle clicks keep the browser's new-tab
// behavior. The expander lazily loads children, nests them to three
// levels, and hands deeper levels to peek.

import { useQuery } from "@tanstack/react-query";
import type { IssueListItem, State } from "@pidash/api-client";
import { ChevronDown, ChevronRight, Loader2 } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { subIssuesQueryOptions } from "../api/queries.js";
import { isRowHighlighted, isTempRowId, rowDomId } from "../freshness/fresh.js";
import type { DisplayFeatureFlags } from "../display/matrix.js";
// The strip brings the property pickers. Its chunk request starts after the
// rows commit, so the list chunk stays cheap to evaluate and rows paint
// before the picker code arrives (desktop warm-start budget). Suspending the
// rows on the chunk instead delayed first paint.
type StripComponent = typeof import("./InlinePropertyStrip.js").InlinePropertyStrip;
let stripRequest: Promise<{ default: StripComponent }> | null = null;
let LoadedStrip: StripComponent | null = null;

function loadStrip(): Promise<{ default: StripComponent }> {
  stripRequest ??= import("./InlinePropertyStrip.js").then((module) => {
    LoadedStrip = module.InlinePropertyStrip;
    return { default: module.InlinePropertyStrip };
  });
  return stripRequest;
}

/** Strip once its chunk has loaded; rows paint without waiting for it. */
function DeferredStrip(props: React.ComponentProps<StripComponent>) {
  // Lazy initializer so a component function is stored, not invoked.
  const [Strip, setStrip] = React.useState<StripComponent | null>(() => LoadedStrip);
  React.useEffect(() => {
    if (Strip !== null) return;
    let live = true;
    const mount = (): void => {
      void loadStrip().then((module) => {
        if (live) setStrip(() => module.default);
      });
    };
    // Mount after the first frame: the pickers are heavy, and mounting them
    // in the commit that paints the rows pushes the first visible frame past
    // the desktop warm-start budget. The strip is visible at rest either way.
    const idle = typeof requestIdleCallback === "function" ? requestIdleCallback(mount, { timeout: 500 }) : null;
    const timer = idle === null ? setTimeout(mount, 0) : null;
    return () => {
      live = false;
      if (idle !== null) cancelIdleCallback(idle);
      if (timer !== null) clearTimeout(timer);
    };
  }, [Strip]);
  return Strip === null ? null : <Strip {...props} />;
}

/** Inline nesting stops at three levels; deeper children open peek. */
export const MAX_INLINE_DEPTH = 3;

export interface ListRowProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  issue: IssueListItem;
  /** 1 for the section's top-level rows. */
  depth: number;
  properties: Record<string, boolean>;
  flags: DisplayFeatureFlags;
  readOnly: boolean;
  states: readonly State[];
  stateNames: ReadonlyMap<string, string>;
  assigneeNames: ReadonlyMap<string, string>;
  labelNames: ReadonlyMap<string, string>;
  cycleNames: ReadonlyMap<string, string>;
  moduleNames: ReadonlyMap<string, string>;
  openCycleIds: ReadonlySet<string>;
  /** Whether the project has an estimate system configured. */
  estimateEnabled: boolean;
  highlightedId: string | null;
  /** Opens the peek panel for the row's key. */
  onOpenPeek: (issueKey: string) => void;
  /** href for the row anchor; the route builds it. */
  hrefFor: (issue: IssueListItem) => string;
  /** Row density from the layout search param (list vs compact). */
  compact?: boolean;
  /** Mount the property pickers on first interaction (virtualized rows). */
  deferPickers?: boolean;
}

export function issueKey(projectIdentifier: string, issue: Pick<IssueListItem, "sequence_id">): string {
  return `${projectIdentifier}-${issue.sequence_id}`;
}

/** Anchor id the row exposes; the block wrapper carries `issue_<id>`. */
export function rowLinkId(issueId: string): string {
  return rowDomId(issueId);
}

/** Wrapper id nesting children under their parent block. */
export function rowBlockId(issueId: string): string {
  return `issue_${issueId}`;
}

/** Chevron + lazy children for one parent row. */
function SubIssueExpander({
  workspaceSlug,
  projectId,
  issue,
  depth,
  projectIdentifier,
  onOpenPeek,
  renderChild,
}: {
  workspaceSlug: string;
  projectId: string;
  issue: IssueListItem;
  depth: number;
  projectIdentifier: string;
  onOpenPeek: (issueKey: string) => void;
  renderChild: (child: IssueListItem, depth: number) => React.ReactNode;
}): React.ReactElement {
  const [expanded, setExpanded] = React.useState(false);
  const atCap = depth >= MAX_INLINE_DEPTH;
  const canExpand = (issue.sub_issues_count ?? 0) > 0 && !issue.is_epic;
  const query = useQuery({
    ...subIssuesQueryOptions(getAppClient(), workspaceSlug, projectId, issue.id),
    enabled: expanded && !atCap && canExpand,
  });

  if (!canExpand) return <span className="grid size-4 shrink-0" aria-hidden="true" />;
  if (atCap) {
    return (
      <span className="grid size-4 shrink-0 place-items-center">
        <button
          type="button"
          aria-label={`Open sub-work items of ${issue.name} in preview`}
          onClick={() => onOpenPeek(issueKey(projectIdentifier, issue))}
          className="grid size-4 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle)"
        >
          <ChevronRight size={12} strokeWidth={2} aria-hidden="true" />
        </button>
      </span>
    );
  }
  const children = query.data?.sub_issues ?? [];
  return (
    <span className="flex min-w-0 flex-col">
      <span className="grid size-4 shrink-0 place-items-center">
        <button
          type="button"
          aria-label={expanded ? `Collapse sub-work items of ${issue.name}` : `Expand sub-work items of ${issue.name}`}
          aria-expanded={expanded}
          onClick={() => setExpanded((open) => !open)}
          className="grid size-4 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle)"
        >
          {query.isFetching ? (
            <Loader2 size={12} strokeWidth={2} className="animate-spin" aria-hidden="true" />
          ) : expanded ? (
            <ChevronDown size={12} strokeWidth={2} aria-hidden="true" />
          ) : (
            <ChevronRight size={12} strokeWidth={2} aria-hidden="true" />
          )}
        </button>
      </span>
      {expanded && children.length > 0 ? (
        <span className="flex flex-col">{children.map((child) => renderChild(child, depth + 1))}</span>
      ) : null}
    </span>
  );
}

function ListRowImpl(props: ListRowProps): React.ReactElement {
  const { issue, highlightedId, depth, projectIdentifier, onOpenPeek, hrefFor, compact = false } = props;
  const temp = isTempRowId(issue.id);
  const highlighted = isRowHighlighted(issue, highlightedId);
  const key = issueKey(projectIdentifier, issue);
  const href = hrefFor(issue);

  function renderChild(child: IssueListItem, childDepth: number): React.ReactNode {
    return <ListRow {...props} key={child.id} issue={child} depth={childDepth} />;
  }

  return (
    <article id={rowBlockId(issue.id)} data-temprow={temp ? "true" : undefined} className="flex flex-col">
      <div
        data-testid="list-row"
        data-highlight={highlighted ? "true" : undefined}
        className={[
          compact ? "h-8" : "h-11",
          "flex items-center gap-(--space-2) border-b border-(--border) px-(--space-3)",
          highlighted ? "bg-(--accent-subtle)" : "",
        ].join(" ")}
      >
        <SubIssueExpander
          workspaceSlug={props.workspaceSlug}
          projectId={props.projectId}
          issue={issue}
          depth={depth}
          projectIdentifier={projectIdentifier}
          onOpenPeek={onOpenPeek}
          renderChild={renderChild}
        />
        <a
          id={rowLinkId(issue.id)}
          data-row-anchor="true"
          href={href}
          aria-label={temp ? `${issue.name}, saving` : key}
          className="flex min-w-0 flex-1 items-center gap-(--space-2) focus-visible:outline-2 focus-visible:outline-(--accent)"
          onClick={(event) => {
            if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
            event.preventDefault();
            if (!temp) onOpenPeek(key);
          }}
        >
          <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
            {temp ? "…" : key}
          </span>
          <p title={issue.name} className="text-body min-w-0 flex-1 truncate text-(--text)">
            {issue.name}
          </p>
        </a>
        {temp ? null : (
          <DeferredStrip
            workspaceSlug={props.workspaceSlug}
            projectId={props.projectId}
            projectIdentifier={projectIdentifier}
            issue={issue}
            properties={props.properties}
            flags={props.flags}
            readOnly={props.readOnly}
            states={props.states}
            stateNames={props.stateNames}
            assigneeNames={props.assigneeNames}
            labelNames={props.labelNames}
            cycleNames={props.cycleNames}
            moduleNames={props.moduleNames}
            openCycleIds={props.openCycleIds}
            estimateEnabled={props.estimateEnabled}
            deferPickers={props.deferPickers ?? false}
          />
        )}
      </div>
    </article>
  );
}

/** Stable props let mounted rows skip virtualizer-driven re-renders. */
export const ListRow = React.memo(ListRowImpl);
