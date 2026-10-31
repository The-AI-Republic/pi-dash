// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One board card (parity/specs/issues.md §5, ISS-033/034, ISS-037). The
// card is a real link to the work item, so middle-click and "open in a
// new tab" keep working; a plain click hands the row to the board, which
// opens the peek panel on a wide viewport and navigates on a narrow one.
// Contents are the identifier, the name clamped to a single line with
// the full name as its tooltip, a quick-action menu that stays visible
// on narrow viewports, and the shared property strip limited to the
// displayed properties. A create the server has not confirmed yet
// renders inert. A press that travels past the threshold hands the card
// to the board's drag instead of opening it; presses starting on the
// card's own controls never drag.

import type { IssueListItem, State } from "@pidash/api-client";
import { Menu, cn } from "@pidash/kit";
import { Copy, ExternalLink, MoreHorizontal } from "lucide-react";
import * as React from "react";

import { useNarrowViewport } from "../../../shared/shell/narrowViewport.js";
import type { DisplayFeatureFlags } from "../display/matrix.js";
import { isPropertyShown } from "../display/matrix.js";
import { isTempRowId } from "../freshness/fresh.js";
import { InlinePropertyStrip } from "../list/InlinePropertyStrip.js";
import { BOARD_DRAG_THRESHOLD_PX } from "./drag.js";

/** Identifier the board prints for one row, e.g. "WEB-12". */
export function cardKey(projectIdentifier: string, issue: Pick<IssueListItem, "sequence_id">): string {
  return `${projectIdentifier}-${issue.sequence_id}`;
}

/** DOM id carrying the card's issue and cell, for the parity driver. */
export function cardDomId(issueId: string, groupKey: string, laneKey: string | null): string {
  return `issue_${issueId}_${groupKey}_${laneKey === null || laneKey === "" ? "null" : laneKey}`;
}

export interface KanbanCardProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  issue: IssueListItem;
  groupKey: string;
  /** Sub-group value for swimlane cards; null in flat mode. */
  laneKey: string | null;
  properties: Record<string, boolean>;
  flags: DisplayFeatureFlags;
  /** Inline editing is off for guests, archived items and completed cycles. */
  readOnly: boolean;
  /** Whether the card's issue is the one open in the peek panel. */
  peeked: boolean;
  estimateEnabled: boolean;
  states: readonly State[];
  stateNames: ReadonlyMap<string, string>;
  assigneeNames: ReadonlyMap<string, string>;
  labelNames: ReadonlyMap<string, string>;
  cycleNames: ReadonlyMap<string, string>;
  moduleNames: ReadonlyMap<string, string>;
  openCycleIds: ReadonlySet<string>;
  timeZone: string | null;
  href: string;
  /** Opens the peek panel for a confirmed card. */
  onOpen: (issue: IssueListItem) => void;
  /** Copies the card's address; the shell toasts the outcome. */
  onCopyLink: (issue: IssueListItem) => void;
  /** False on read-only boards and touch layouts; unconfirmed rows never drag. */
  draggable?: boolean | undefined;
  /** Hands the card to the board's drag once the press travels. */
  onDragStart?: ((issue: IssueListItem, event: React.PointerEvent<HTMLElement>) => void) | undefined;
  /** True while this card is the drag source. */
  dragging?: boolean | undefined;
  /** True briefly after the card lands from a drop. */
  dropped?: boolean | undefined;
}

/** Card controls a drag press ignores (the card link itself may drag). */
const DRAG_DEAD_CONTROLS = "button, [role='menuitem'], input, select, textarea";

export function KanbanCard(props: KanbanCardProps): React.ReactElement {
  const { issue, projectIdentifier, groupKey, laneKey, properties, peeked } = props;
  const narrow = useNarrowViewport();
  const unconfirmed = isTempRowId(issue.id);
  const key = cardKey(projectIdentifier, issue);
  const showKey = isPropertyShown(properties, "key");
  const articleRef = React.useRef<HTMLElement | null>(null);
  const pressRef = React.useRef<{ x: number; y: number } | null>(null);
  const dragListenersRef = React.useRef<(() => void) | null>(null);
  const draggedRef = React.useRef(false);
  const highlighted = peeked || props.dropped === true;

  React.useEffect(
    () => () => {
      dragListenersRef.current?.();
    },
    []
  );

  React.useEffect(() => {
    // Environments without layout (tests, SSR snapshots) lack the call.
    if (props.dropped === true) articleRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [props.dropped]);

  function stopTracking(): void {
    pressRef.current = null;
    dragListenersRef.current?.();
    dragListenersRef.current = null;
  }

  /**
   * Once a press starts, watch the whole document rather than just the
   * card. A real drag leaves the card on its way to the destination, so
   * card-local pointermove listeners would go quiet before the movement
   * threshold is crossed.
   */
  function trackPointer(): void {
    const onDragStart = props.onDragStart;
    if (onDragStart === undefined) return;
    // Deliberately no pointer capture: capture keeps up events on this
    // card, so the board would never see the release. The board resolves
    // the drop from the document instead.
    const onMove = (moveEvent: PointerEvent): void => {
      const press = pressRef.current;
      if (press === null) return;
      const dx = moveEvent.clientX - press.x;
      const dy = moveEvent.clientY - press.y;
      if (Math.hypot(dx, dy) < BOARD_DRAG_THRESHOLD_PX) return;
      pressRef.current = null;
      draggedRef.current = true;
      dragListenersRef.current?.();
      dragListenersRef.current = null;
      onDragStart(issue, moveEvent as unknown as React.PointerEvent<HTMLElement>);
    };
    const onEnd = (): void => stopTracking();
    globalThis.document.addEventListener("pointermove", onMove);
    globalThis.document.addEventListener("pointerup", onEnd);
    globalThis.document.addEventListener("pointercancel", onEnd);
    dragListenersRef.current = () => {
      globalThis.document.removeEventListener("pointermove", onMove);
      globalThis.document.removeEventListener("pointerup", onEnd);
      globalThis.document.removeEventListener("pointercancel", onEnd);
    };
  }

  return (
    <article
      ref={articleRef}
      data-testid="kanban-card"
      data-issue-id={issue.id}
      data-group-key={groupKey}
      data-lane-key={laneKey ?? ""}
      data-peeked={peeked ? "true" : undefined}
      data-unconfirmed={unconfirmed ? "true" : undefined}
      data-dragging={props.dragging === true ? "true" : undefined}
      onPointerDown={(event) => {
        if (props.draggable !== true || unconfirmed) return;
        if ((event.target as Element).closest(DRAG_DEAD_CONTROLS) !== null) return;
        draggedRef.current = false;
        pressRef.current = { x: event.clientX, y: event.clientY };
        trackPointer();
      }}
      onPointerUp={() => {
        stopTracking();
      }}
      onPointerCancel={() => {
        stopTracking();
      }}
      className={cn(
        "group/card flex flex-col gap-(--space-1) rounded-(--radius-control) border bg-(--surface) p-(--space-3) text-left",
        highlighted ? "border-(--accent) ring-1 ring-(--accent)" : "border-(--border) hover:border-(--text-muted)",
        unconfirmed ? "animate-pulse" : "",
        props.dragging === true ? "opacity-50" : ""
      )}
    >
      <div className="flex items-center gap-(--space-2)">
        <a
          id={cardDomId(issue.id, groupKey, laneKey)}
          data-testid="kanban-card-link"
          data-row-anchor="true"
          href={unconfirmed ? undefined : props.href}
          aria-label={unconfirmed ? `${issue.name}, saving` : key}
          aria-disabled={unconfirmed ? true : undefined}
          tabIndex={unconfirmed ? -1 : undefined}
          draggable={false}
          onDragStart={(event) => {
            // A native link drag would swallow the pointerup that ends our
            // drag, leaving the board's drag state armed for a later click.
            event.preventDefault();
            stopTracking();
            draggedRef.current = false;
          }}
          onClick={(event) => {
            if (draggedRef.current) {
              draggedRef.current = false;
              return;
            }
            if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
            event.preventDefault();
            if (!unconfirmed) props.onOpen(issue);
          }}
          className="flex min-w-0 flex-1 flex-col gap-(--space-1) focus-visible:outline-2 focus-visible:outline-(--accent)"
        >
          {showKey ? (
            <span className="text-caption font-[family-name:var(--font-mono)] text-(--text-muted)">
              {unconfirmed ? "…" : key}
            </span>
          ) : null}
          <span
            data-testid="kanban-card-name"
            title={issue.name}
            className="text-body line-clamp-1 block truncate text-(--text)"
          >
            {issue.name}
          </span>
        </a>
        {unconfirmed ? null : (
          <span
            data-testid="kanban-card-actions"
            className={cn("shrink-0", narrow ? "block" : "hidden group-focus-within/card:block group-hover/card:block")}
          >
            <Menu
              trigger={
                <button
                  type="button"
                  aria-label={`Actions for ${issue.name}`}
                  className="grid size-6 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                  onClick={(event) => event.stopPropagation()}
                >
                  <MoreHorizontal size={14} strokeWidth={2} aria-hidden="true" />
                </button>
              }
              items={[
                {
                  label: "Open work item",
                  icon: <ExternalLink size={14} strokeWidth={1.5} aria-hidden="true" />,
                  onSelect: () => {
                    props.onOpen(issue);
                  },
                },
                {
                  label: "Copy link",
                  icon: <Copy size={14} strokeWidth={1.5} aria-hidden="true" />,
                  onSelect: () => {
                    props.onCopyLink(issue);
                  },
                },
              ]}
            />
          </span>
        )}
      </div>
      {unconfirmed ? null : (
        <div data-testid="kanban-card-properties" className="flex flex-wrap items-center gap-(--space-2)">
          <InlinePropertyStrip
            workspaceSlug={props.workspaceSlug}
            projectId={props.projectId}
            projectIdentifier={projectIdentifier}
            issue={issue}
            properties={properties}
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
            deferPickers
            timeZone={props.timeZone}
          />
        </div>
      )}
    </article>
  );
}
