// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard widget stack (SHELL-021–023; written from parity/specs/shell.md
// §2). The header pairs the home breadcrumb with the widget-settings
// shortcut; below it the staged shell skeleton yields to the enabled
// sections in map order. The quicklinks section, the manage-widgets dialog
// and the all-off illustration arrive through slots owned by NEWFRONT-279;
// unknown keys render nothing.

import * as React from "react";
import { SlidersHorizontal } from "lucide-react";

import { IconButton, Skeleton } from "@pidash/kit";
import type { ProjectList } from "@pidash/api-client";
import { useWidgetPrefs } from "../api/widgetPrefs.js";
import { orderWidgetPrefs, visibleWidgetPrefs, widgetSectionKind } from "../lib/widgets.js";
import { RecentsWidget } from "./RecentsWidget.js";

export interface ManageWidgetsControls {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export interface HomeWidgetStackProps {
  workspaceSlug: string;
  /** Workspace projects for the recents identifier fallbacks; the route reads them. */
  projects: ProjectList | undefined;
  /** The quicklinks section, owned by NEWFRONT-279. */
  quicklinks?: React.ReactNode;
  /** The manage-widgets dialog, owned by NEWFRONT-279. */
  manageWidgets?: (controls: ManageWidgetsControls) => React.ReactNode;
  /** The all-widgets-off illustration, owned by NEWFRONT-279. */
  emptyState?: React.ReactNode;
}

function DashboardSkeleton(): React.ReactElement {
  return (
    <div data-testid="dashboard-skeleton" aria-busy="true" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <Skeleton width="8rem" height="20px" />
        <Skeleton height="44px" />
        <Skeleton height="44px" />
        <Skeleton height="44px" />
      </div>
      <div className="flex flex-col gap-(--space-2)">
        <Skeleton width="8rem" height="20px" />
        <Skeleton height="44px" />
        <Skeleton height="44px" />
      </div>
    </div>
  );
}

export function HomeWidgetStack({
  workspaceSlug,
  projects,
  quicklinks,
  manageWidgets,
  emptyState,
}: HomeWidgetStackProps): React.ReactElement {
  const [manageOpen, setManageOpen] = React.useState(false);
  const prefs = useWidgetPrefs(workspaceSlug);
  const sections = React.useMemo(() => visibleWidgetPrefs(orderWidgetPrefs(prefs.data ?? [])), [prefs.data]);
  const hasKnownSection = sections.some((section) => widgetSectionKind(section.key) !== "other");

  return (
    <div id="home-widget-stack" className="flex flex-col gap-(--space-6)">
      <div className="flex items-center justify-between gap-(--space-3)">
        <nav aria-label="Breadcrumb">
          <ol>
            <li aria-current="page" className="text-h3 text-(--text)">
              Home
            </li>
          </ol>
        </nav>
        <IconButton
          label="Manage widgets"
          onClick={() => {
            setManageOpen(true);
          }}
        >
          <SlidersHorizontal size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
      {prefs.data === undefined ? (
        <DashboardSkeleton />
      ) : hasKnownSection ? (
        sections.map((section) => {
          switch (widgetSectionKind(section.key)) {
            case "recents":
              return <RecentsWidget key={section.key} workspaceSlug={workspaceSlug} projects={projects} />;
            case "quicklinks":
              return <React.Fragment key={section.key}>{quicklinks}</React.Fragment>;
            default:
              return null;
          }
        })
      ) : (
        (emptyState ?? null)
      )}
      {manageWidgets?.({ open: manageOpen, onOpenChange: setManageOpen })}
    </div>
  );
}
