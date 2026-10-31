// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One list row (parity/specs/issues.md §2, ISS-013). The row is a real
// anchor to the browse route; a plain click opens peek instead of
// navigating, while modified/middle clicks keep the browser's new-tab
// behavior. The expander lazily loads children, nests them to three
// levels, and hands deeper levels to peek. Each live row carries the
// shared quick-action menu twice: a hover ellipsis and the right-click
// menu (ISS-060); children inherit the parent's menu context.

import { useQuery } from "@tanstack/react-query";
import type { IssueListItem, State } from "@pidash/api-client";
import { ChevronDown, ChevronRight, Loader2 } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { subIssuesQueryOptions } from "../api/subIssuePage.js";
import { isRowHighlighted, isTempRowId, rowDomId } from "../freshness/fresh.js";
import type { DisplayFeatureFlags } from "../display/matrix.js";
import { ForeignRowSelectionHint, RowSelectionCheckbox } from "../bulk/SelectionCheckbox.js";
import { SELECTION_ROW_ATTR } from "../bulk/useBulkSelection.js";
import { RowContextMenu, RowMenuButton } from "../menus/RowQuickActions.js";
import type { QuickActionContext } from "../menus/model.js";
import { quickActionTarget, useQuickActions } from "../menus/useQuickActions.js";
// The strip brings the property pickers. The route primes its chunk with
// its own module, so it is usually resolved by first render and rows mount
// full; when it is not, rows paint as shells and mount the strip on idle
// (NEWFRONT-911). Suspending the rows on the chunk instead delayed first
// paint.
import { loadedInlineStrip, loadInlineStrip, type InlineStripComponent } from "./stripLazy.js";

/** Strip once its chunk has loaded; rows paint without waiting for it. */
function DeferredStrip(props: React.ComponentProps<InlineStripComponent>) {
  // Lazy initializer so a component function is stored, not invoked.
  const [Strip, setStrip] = React.useState<InlineStripComponent | null>(() => loadedInlineStrip());
  React.useEffect(() => {
    if (Strip !== null) return;
    let live = true;
    const mount = (): void => {
      void loadInlineStrip().then((module) => {
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
  /** Resolves the key prefix for a nested child that may live elsewhere. */
  projectIdentifierFor?: ((issue: IssueListItem) => string) | undefined;
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
  /** Opens the peek panel for the row's work-item id. */
  onOpenPeek: (issueId: string, title: string) => void;
  /** href for the row anchor; the route builds it. */
  hrefFor: (issue: IssueListItem) => string;
  /** Row density from the layout search param (list vs compact). */
  compact?: boolean;
  /** Mount the property pickers on first interaction (virtualized rows). */
  deferPickers?: boolean;
  /** Show the sub-work-item expander (Display → Show sub-work items). */
  showSubIssues?: boolean;
  /** Project timezone for the overdue due-date check. */
  timeZone?: string | null;
  /** Selection control, when the list enables bulk selection. */
  selected?: boolean | undefined;
  active?: boolean | undefined;
  /** Resolves a row's selection state, including recursively rendered children. */
  selectionSelectedFor?: ((id: string) => boolean) | undefined;
  selectionActiveFor?: ((id: string) => boolean) | undefined;
  onSelectionToggle?: ((id: string, shiftKey: boolean) => void) | undefined;
  /** Enables the checkbox; a foreign-project row gets a disabled hint. */
  selectionAvailable?: boolean | undefined;
  /** Reports an expansion change so the layout can flatten nested rows. */
  onExpandedChange?: ((id: string, expanded: boolean) => void) | undefined;
  /** Reports the loaded children of an expanded row to the layout. */
  onChildrenChange?: ((id: string, children: readonly IssueListItem[]) => void) | undefined;
  /** Shared expansion state, supplied when the layout flattens child rows. */
  selectionTree?:
    | {
        expanded: ReadonlySet<string>;
        children: ReadonlyMap<string, readonly IssueListItem[]>;
      }
    | undefined;
  /** Which menu context the row opens (project, cycle, module or archived). */
  menuContext?: QuickActionContext | undefined;
  /** Cycle or module the row sits in, for the remove entry and edit prefill. */
  menuCycleId?: string | null | undefined;
  menuModuleId?: string | null | undefined;
  /** The viewer may write on the project; absent resolves from the read. */
  menuCanWrite?: boolean | undefined;
  /** Called after any menu write lands (the route refreshes its own reads). */
  onMenuChanged?: (() => void) | undefined;
  /** Render the row's quick-action menu. */
  showMenu?: boolean | undefined;
  /**
   * Address the menu's copy-link and open-in-new-tab hand out. Archived
   * rows pass their detail address (parity/specs/archives.md §3.3);
   * absent keeps the live browse address.
   */
  menuLinkHref?: string | undefined;
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

/** Chevron toggle for one parent row; the children render under the row. */
function SubIssueToggle({
  issue,
  expanded,
  fetching,
  depth,
  onToggle,
  onOpenPeek,
  showSubIssues,
}: {
  issue: IssueListItem;
  expanded: boolean;
  fetching: boolean;
  depth: number;
  onToggle: () => void;
  onOpenPeek: (issueId: string, title: string) => void;
  showSubIssues: boolean;
}): React.ReactElement {
  const atCap = depth >= MAX_INLINE_DEPTH;
  const canExpand = (issue.sub_issues_count ?? 0) > 0 && !issue.is_epic;

  // Both a hidden toggle and a leaf keep the same gutter, so titles align.
  if (!showSubIssues || !canExpand) return <span className="grid size-4 shrink-0" aria-hidden="true" />;
  if (atCap) {
    return (
      <span className="grid size-4 shrink-0 place-items-center">
        <button
          type="button"
          aria-label={`Open sub-work items of ${issue.name} in preview`}
          onClick={() => onOpenPeek(issue.id, issue.name)}
          className="grid size-4 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle)"
        >
          <ChevronRight size={12} strokeWidth={2} aria-hidden="true" />
        </button>
      </span>
    );
  }
  return (
    <span className="grid size-4 shrink-0 place-items-center">
      <button
        type="button"
        aria-label={expanded ? `Collapse sub-work items of ${issue.name}` : `Expand sub-work items of ${issue.name}`}
        aria-expanded={expanded}
        onClick={onToggle}
        className="grid size-4 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle)"
      >
        {fetching ? (
          <Loader2 size={12} strokeWidth={2} className="animate-spin" aria-hidden="true" />
        ) : expanded ? (
          <ChevronDown size={12} strokeWidth={2} aria-hidden="true" />
        ) : (
          <ChevronRight size={12} strokeWidth={2} aria-hidden="true" />
        )}
      </button>
    </span>
  );
}

function ListRowImpl(props: ListRowProps): React.ReactElement {
  const {
    issue,
    highlightedId,
    depth,
    projectIdentifier,
    projectIdentifierFor,
    onOpenPeek,
    hrefFor,
    compact = false,
    showSubIssues = true,
    timeZone = null,
    selected = false,
    active = false,
    selectionSelectedFor,
    selectionActiveFor,
    onSelectionToggle,
    selectionAvailable = true,
    onExpandedChange,
    onChildrenChange,
    selectionTree,
    showMenu = true,
  } = props;
  const temp = isTempRowId(issue.id);
  const highlighted = isRowHighlighted(issue, highlightedId);
  const key = issueKey(projectIdentifierFor?.(issue) ?? projectIdentifier, issue);
  const isSelected = selectionSelectedFor?.(issue.id) ?? selected;
  const isActive = selectionActiveFor?.(issue.id) ?? active;
  const href = hrefFor(issue);
  const [uncontrolledExpanded, setUncontrolledExpanded] = React.useState(false);
  const expanded = selectionTree === undefined ? uncontrolledExpanded : selectionTree.expanded.has(issue.id);
  const treeChildren = selectionTree?.children.get(issue.id);
  const atCap = depth >= MAX_INLINE_DEPTH;
  const canExpand = (issue.sub_issues_count ?? 0) > 0 && !issue.is_epic;
  const query = useQuery({
    ...subIssuesQueryOptions(getAppClient(), props.workspaceSlug, props.projectId, issue.id),
    // Hiding the expander must also stop any in-flight child read, so the
    // display flag gates the query and the render together.
    enabled: showSubIssues && expanded && !atCap && canExpand,
  });
  const children = treeChildren ?? query.data?.sub_issues ?? [];
  const menu = useQuickActions(
    temp || !showMenu
      ? null
      : {
          workspaceSlug: props.workspaceSlug,
          target: quickActionTarget(issue, projectIdentifier),
          context: props.menuContext ?? "project",
          cycleId: props.menuCycleId ?? null,
          moduleId: props.menuModuleId ?? null,
          ...(props.menuCanWrite === undefined ? {} : { canWrite: props.menuCanWrite }),
          ...(props.menuLinkHref === undefined ? {} : { linkHref: props.menuLinkHref }),
          onChanged: props.onMenuChanged,
        }
  );
  const toggleExpanded = (): void => {
    const next = !expanded;
    if (selectionTree === undefined) setUncontrolledExpanded(next);
    onExpandedChange?.(issue.id, next);
  };

  React.useEffect(() => {
    if (expanded && query.data !== undefined) onChildrenChange?.(issue.id, query.data.sub_issues ?? []);
  }, [expanded, issue.id, onChildrenChange, query.data]);

  const rowBody = (
    <div
      data-testid="list-row"
      {...{ [SELECTION_ROW_ATTR]: issue.id }}
      data-highlight={highlighted ? "true" : undefined}
      data-active={isActive ? "true" : undefined}
      className={[
        compact ? "h-8" : "h-11",
        "group/row flex items-center gap-(--space-2) border-b border-(--border) px-(--space-3)",
        highlighted ? "bg-(--accent-subtle)" : "",
        isActive ? "ring-1 ring-inset ring-(--accent)" : "",
      ].join(" ")}
    >
      {onSelectionToggle === undefined ? null : selectionAvailable ? (
        <RowSelectionCheckbox
          checked={isSelected}
          label={`Select ${key}`}
          onChange={(event) => onSelectionToggle(issue.id, event.shiftKey)}
        />
      ) : (
        <ForeignRowSelectionHint label={`Cannot select ${key}`} />
      )}
      <SubIssueToggle
        issue={issue}
        expanded={expanded}
        fetching={query.isFetching}
        depth={depth}
        onToggle={toggleExpanded}
        onOpenPeek={onOpenPeek}
        showSubIssues={showSubIssues}
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
          if (!temp) onOpenPeek(issue.id, issue.name);
        }}
      >
        {props.properties["key"] === false ? null : (
          <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
            {temp ? "…" : key}
          </span>
        )}
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
          timeZone={timeZone}
        />
      )}
      {menu === null ? null : (
        <RowMenuButton
          state={menu}
          issueKey={key}
          className="opacity-0 transition-opacity group-hover/row:opacity-100 focus-within:opacity-100"
        />
      )}
    </div>
  );

  return (
    <article id={rowBlockId(issue.id)} data-temprow={temp ? "true" : undefined} className="flex flex-col">
      {menu === null ? rowBody : <RowContextMenu state={menu} trigger={rowBody} />}
      {expanded && children.length > 0 ? (
        <div data-testid="list-row-children" className="flex flex-col pl-(--space-4)">
          {children.map((child) => (
            <ListRow
              {...props}
              key={child.id}
              issue={child}
              depth={depth + 1}
              selectionAvailable={(props.selectionAvailable ?? true) && child.project_id === props.projectId}
            />
          ))}
        </div>
      ) : null}
    </article>
  );
}

/** Stable props let mounted rows skip virtualizer-driven re-renders. */
export const ListRow = React.memo(ListRowImpl);
