// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Display options header control (parity/specs/issues.md §9, ISS-075,
// ISS-106). The header "Display" button plus its popover panel, wired to
// the per-user per-entity preferences: every change PATCHes the server
// record and refetches the entity lists under the mutation loader. The
// control never reads or writes the URL — display state is personal, not
// deep-linkable — and resolves the same active layout the root renders.
// The panel rides React.lazy so the route chunk carries only the trigger.

import { Button, Popover } from "@pidash/kit";
import { SlidersHorizontal } from "lucide-react";
import * as React from "react";

import { resolveActiveLayout, type LayoutKind } from "../layouts/kinds.js";
import { selectDisplayPrefs, useEntityPrefs, useUpdateDisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import type { DisplaySwitchKey } from "./matrix.js";

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
}

export function DisplayControl({
  workspaceSlug,
  entity,
  offered,
  hideCycle,
  hideModule,
}: DisplayControlProps): React.ReactElement {
  const { data: record } = useEntityPrefs(workspaceSlug, entity);
  const { updateDisplayPrefs } = useUpdateDisplayPrefs(workspaceSlug, entity);
  const prefs = record ? selectDisplayPrefs(record) : null;
  const active = prefs ? resolveActiveLayout(prefs.layout, offered) : null;
  const toggleSwitch = (key: DisplaySwitchKey): void => {
    if (!prefs) return;
    if (key === "sub_issue") updateDisplayPrefs({ showSubIssues: !prefs.showSubIssues });
    else updateDisplayPrefs({ showEmptyGroups: !prefs.showEmptyGroups });
  };
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
      {prefs && active ? (
        <React.Suspense
          fallback={
            <p className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">Loading display options…</p>
          }
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
            onGroupBy={(dimension) => updateDisplayPrefs({ groupBy: dimension })}
            onSubGroupBy={(dimension) => updateDisplayPrefs({ subGroupBy: dimension })}
            onOrderBy={(order) => updateDisplayPrefs({ orderBy: order })}
            onToggleSwitch={toggleSwitch}
            onToggleProperty={(key, shown) => updateDisplayPrefs({ properties: { ...prefs.properties, [key]: shown } })}
          />
        </React.Suspense>
      ) : (
        <p className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">Loading display options…</p>
      )}
    </Popover>
  );
}
