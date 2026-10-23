// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// View switcher (parity/specs/views.md §7, VIEW-023). The last breadcrumb
// is a dropdown over the project's fetched view list, in the same order
// the list screen shows. It searches by name, marks the open view, and
// navigates in the same tab on pick; nothing here writes.

import type { SavedView } from "@pidash/api-client";
import { Button, Combobox, type ComboboxOption } from "@pidash/kit";
import { ChevronDown, Layers } from "lucide-react";
import * as React from "react";

import { projectMark } from "../../../shared/pickers/projectIcon.js";
import { ProjectMarkIcon } from "../../../shared/pickers/ProjectMarkIcon.js";

export interface SavedViewSwitcherProps {
  /** The open record; its name is the trigger's accessible name. */
  current: Pick<SavedView, "id" | "name" | "logo_props">;
  /** Visible views, already in the list screen's order. */
  views: readonly SavedView[];
  loading: boolean;
  failed: boolean;
  onPick: (viewId: string) => void;
}

/** The leading mark for one option: the view's stored logo or the default. */
export function ViewSwitcherMark({ view }: { view: SavedView }): React.ReactElement {
  const mark = projectMark(view.logo_props);
  if (mark.kind === "emoji") {
    return <span className="grid size-4 shrink-0 place-items-center text-[16px] leading-none">{mark.glyph}</span>;
  }
  if (mark.kind === "icon") return <ProjectMarkIcon mark={mark} identifier={view.name} size={16} />;
  // The default mark is the views glyph, never the name's initial: the
  // option's own label supplies the name.
  return <Layers size={16} strokeWidth={1.5} />;
}

export function viewSwitcherOptions(views: readonly SavedView[]): ComboboxOption[] {
  return views.map((view) => ({
    id: view.id,
    label: view.name,
    icon: <ViewSwitcherMark view={view} />,
  }));
}

export function SavedViewSwitcher({
  current,
  views,
  loading,
  failed,
  onPick,
}: SavedViewSwitcherProps): React.ReactElement {
  const options = React.useMemo(() => viewSwitcherOptions(views), [views]);
  return (
    <Combobox
      trigger={
        <Button variant="ghost" size="small" className="max-w-64" aria-label={current.name}>
          <span className="truncate">{current.name}</span>
          <ChevronDown size={14} strokeWidth={1.5} aria-hidden="true" />
        </Button>
      }
      options={options}
      value={current.id}
      onSelect={onPick}
      listLabel="Saved views"
      searchLabel="Search views"
      emptyText="No views match that name."
      loading={loading}
      loadingText="Loading views"
      {...(failed ? { errorText: "Views could not be loaded." } : {})}
    />
  );
}
