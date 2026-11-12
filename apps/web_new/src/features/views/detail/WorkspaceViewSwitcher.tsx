// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace view switcher (parity/specs/views.md §11, VIEW-038). The
// last breadcrumb is a dropdown over the four static defaults, then the
// fetched customs in list order. It searches by name, marks the open
// view, and navigates in the same tab on pick; nothing here writes.

import type { SavedView } from "@pidash/api-client";
import { Button, Combobox, type ComboboxOption } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { WORKSPACE_DEFAULT_VIEWS } from "../lib/workspaceViews.js";
import { ViewSwitcherMark } from "./SavedViewSwitcher.js";

export interface WorkspaceViewSwitcherProps {
  /** Route key of a default or id of a custom; the marked option. */
  currentKey: string;
  /** Displayed label; the trigger's accessible name. */
  currentName: string;
  /** Visible customs, already in the list screen's order. */
  customs: readonly SavedView[];
  loading: boolean;
  failed: boolean;
  /** Route key or id the detail navigates to in the same tab. */
  onPick: (viewId: string) => void;
}

/** Defaults first with the default mark, then the customs with their own. */
export function workspaceSwitcherOptions(customs: readonly SavedView[]): ComboboxOption[] {
  return [
    ...WORKSPACE_DEFAULT_VIEWS.map((view) => ({
      id: view.key,
      label: view.label,
      icon: <ViewSwitcherMark view={{ name: view.label, logo_props: {} }} />,
    })),
    ...customs.map((view) => ({
      id: view.id,
      label: view.name,
      icon: <ViewSwitcherMark view={view} />,
    })),
  ];
}

export function WorkspaceViewSwitcher({
  currentKey,
  currentName,
  customs,
  loading,
  failed,
  onPick,
}: WorkspaceViewSwitcherProps): React.ReactElement {
  const options = React.useMemo(() => workspaceSwitcherOptions(customs), [customs]);
  return (
    <Combobox
      trigger={
        <Button variant="ghost" size="small" className="max-w-64" aria-label={currentName}>
          <span className="truncate">{currentName}</span>
          <ChevronDown size={14} strokeWidth={1.5} aria-hidden="true" />
        </Button>
      }
      options={options}
      value={currentKey}
      onSelect={onPick}
      listLabel="Workspace views"
      searchLabel="Search views"
      emptyText="No views match that name."
      loading={loading}
      loadingText="Loading views"
      {...(failed ? { errorText: "Views could not be loaded." } : {})}
    />
  );
}
