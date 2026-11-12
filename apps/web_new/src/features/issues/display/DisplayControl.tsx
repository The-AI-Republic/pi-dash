// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Display options header control (parity/specs/issues.md §9, ISS-075,
// ISS-106). The header "Display" button plus its popover panel, wired to
// the per-user per-entity preferences: every change PATCHes the server
// record and refetches the entity lists under the mutation loader. The
// control never reads or writes the URL — display state is personal, not
// deep-linkable — and resolves the same active layout the root renders.
// A saved view passes its own stored display and an `onChange` callback, so
// the row edits the view's settings locally (Update view persists them)
// instead of hidden personal preferences.
// The panel rides React.lazy so the route chunk carries only the trigger.

import { Button, Popover } from "@pidash/kit";
import { SlidersHorizontal } from "lucide-react";
import * as React from "react";

import { resolveActiveLayout, type LayoutKind } from "../layouts/kinds.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import { mergeDisplayPrefs, selectDisplayPrefs, useEntityPrefs, useUpdateDisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import type { LayoutDisplayMatrix as DisplayMatrix } from "./matrix.js";

const LazyDisplayPanel = React.lazy(() =>
  import("./DisplayPanel.js").then((module) => ({ default: module.DisplayPanel }))
);

export interface DisplayControlProps {
  workspaceSlug: string;
  entity: IssuesEntity;
  /** Registered layouts; archived contexts pass list only. */
  offered: readonly LayoutKind[];
  /** Cycle/module controls hide while their project features are off. */
  hideCycle: boolean;
  hideModule: boolean;
  /** Saved view: render and edit these settings instead of the personal record. */
  display?: DisplayPrefs | undefined;
  /** Saved view: called with the complete next settings instead of a preference write. */
  onDisplayChange?: ((next: DisplayPrefs) => void) | undefined;
  /** Surface matrix override; archived passes its list-only matrix. */
  matrix?: DisplayMatrix | undefined;
}

interface LiveDisplayPanelProps {
  prefs: DisplayPrefs;
  offered: readonly LayoutKind[];
  hideCycle: boolean;
  hideModule: boolean;
  entity: IssuesEntity;
  matrix: DisplayMatrix | undefined;
  onDisplayChange: ((next: DisplayPrefs) => void) | undefined;
  updateDisplayPrefs: ReturnType<typeof useUpdateDisplayPrefs>["updateDisplayPrefs"];
}

function LiveDisplayPanel({
  prefs,
  offered,
  hideCycle,
  hideModule,
  entity,
  matrix,
  onDisplayChange,
  updateDisplayPrefs,
}: LiveDisplayPanelProps): React.ReactElement {
  const write = (partial: Partial<DisplayPrefs>): void => {
    if (onDisplayChange !== undefined) onDisplayChange(mergeDisplayPrefs(prefs, partial));
    else updateDisplayPrefs(partial);
  };
  const active = resolveActiveLayout(prefs.layout, offered);
  return (
    <React.Suspense
      fallback={<p className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">Loading display options…</p>}
    >
      <LazyDisplayPanel
        layout={active}
        state={{
          groupBy: prefs.groupBy,
          subGroupBy: prefs.subGroupBy,
          orderBy: prefs.orderBy,
          showEmptyGroups: prefs.showEmptyGroups,
          showSubIssues: prefs.showSubIssues,
          properties: prefs.properties,
        }}
        hideCycle={hideCycle}
        hideModule={hideModule}
        isEpic={entity.scope === "epics"}
        {...(matrix === undefined ? {} : { matrix })}
        onGroupBy={(dimension) => write({ groupBy: dimension })}
        onSubGroupBy={(dimension) => write({ subGroupBy: dimension })}
        onOrderBy={(order) => write({ orderBy: order })}
        onToggleSwitch={(key, next) =>
          key === "sub_issue" ? write({ showSubIssues: next }) : write({ showEmptyGroups: next })
        }
        onToggleProperty={(key, shown) => write({ properties: { ...prefs.properties, [key]: shown } })}
      />
    </React.Suspense>
  );
}

function DisplayPopover({
  prefs,
  offered,
  hideCycle,
  hideModule,
  entity,
  matrix,
  onDisplayChange,
  updateDisplayPrefs,
}: LiveDisplayPanelProps): React.ReactElement {
  return (
    <Popover
      trigger={
        <Button variant="secondary" size="small" data-testid="display-options-trigger">
          <SlidersHorizontal size={14} strokeWidth={2} aria-hidden="true" />
          <span>Display</span>
        </Button>
      }
      className="max-h-[24rem] w-80 overflow-y-auto"
    >
      <LiveDisplayPanel
        prefs={prefs}
        offered={offered}
        hideCycle={hideCycle}
        hideModule={hideModule}
        entity={entity}
        matrix={matrix}
        onDisplayChange={onDisplayChange}
        updateDisplayPrefs={updateDisplayPrefs}
      />
    </Popover>
  );
}

/** Local-display control: the caller owns persistence (saved views, archives). */
function LocalDisplayControl({
  offered,
  hideCycle,
  hideModule,
  entity,
  display,
  onDisplayChange,
  matrix,
}: DisplayControlProps & { display: DisplayPrefs }): React.ReactElement {
  return (
    <DisplayPopover
      prefs={display}
      offered={offered}
      hideCycle={hideCycle}
      hideModule={hideModule}
      entity={entity}
      matrix={matrix}
      onDisplayChange={onDisplayChange}
      updateDisplayPrefs={() => undefined}
    />
  );
}

/** Server-display control: changes PATCH the entity's user-properties record. */
function ServerDisplayControl({
  workspaceSlug,
  entity,
  offered,
  hideCycle,
  hideModule,
  matrix,
}: DisplayControlProps): React.ReactElement {
  const { data: record } = useEntityPrefs(workspaceSlug, entity, true);
  const { updateDisplayPrefs } = useUpdateDisplayPrefs(workspaceSlug, entity);
  const prefs = record ? selectDisplayPrefs(record) : null;
  if (prefs === null) {
    return (
      <Popover
        trigger={
          <Button variant="secondary" size="small" data-testid="display-options-trigger">
            <SlidersHorizontal size={14} strokeWidth={2} aria-hidden="true" />
            <span>Display</span>
          </Button>
        }
        className="max-h-[24rem] w-80 overflow-y-auto"
      >
        <p className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">Loading display options…</p>
      </Popover>
    );
  }
  return (
    <DisplayPopover
      prefs={prefs}
      offered={offered}
      hideCycle={hideCycle}
      hideModule={hideModule}
      entity={entity}
      matrix={matrix}
      onDisplayChange={undefined}
      updateDisplayPrefs={updateDisplayPrefs}
    />
  );
}

export function DisplayControl(props: DisplayControlProps): React.ReactElement {
  if (props.display !== undefined) return <LocalDisplayControl {...props} display={props.display} />;
  return <ServerDisplayControl {...props} />;
}
