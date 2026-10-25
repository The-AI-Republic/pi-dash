// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module row in the list layout (parity/specs/modules.md §3.5, MOD-012).
// The row's identity is the progress marker, the name as a real anchor to
// the detail page, and the info control that opens the peek. Archived rows
// reuse the same component in archived mode: a press on the row opens the
// archived peek and leaves the address where it is, so nothing pretends an
// archived module still has a live detail page.
//
// Inline status, dates, lead, favorite and the quick menu arrive through
// the accessory slots owned by later slices, so this file never grows a
// second copy of those controls. The controls live outside the name anchor:
// pressing one never navigates.

import { Link } from "@tanstack/react-router";
import { IconButton, ProgressRing } from "@pidash/kit";
import type { ModuleRow } from "@pidash/api-client";
import { AlertCircle, Check, Info } from "lucide-react";
import * as React from "react";

import { moduleDetailHref } from "../lib/moduleList.js";
import { modulePressHitControl } from "../lib/modulePress.js";
import { modulePercentFloor, moduleProgressMarker } from "../lib/moduleIdentity.js";

export interface ModuleListRowProps {
  workspaceSlug: string;
  projectId: string;
  module: ModuleRow;
  /** Reading-order extras ahead of the name. Slot owner: NEWFRONT-796. */
  leading?: React.ReactNode;
  /** Inline controls after the name. Slot owners: NEWFRONT-796/797/800. */
  trailing?: React.ReactNode;
  /**
   * Archived mode: a press on the row (or its name link) opens the peek
   * instead of navigating, and the row marks the module as archived. Slice
   * G mounts the row this way on the archives screen.
   */
  archived?: boolean | undefined;
  /** Open this module's peek from the row body or the info control. */
  onOpenPeek?: ((moduleId: string) => void) | undefined;
  /**
   * The narrow-screen quick menu. It replaces the hover-only actions on a
   * phone; slice H supplies the entries, this row supplies the slot.
   */
  menu?: React.ReactNode;
  /** Keep the peek trigger and menu visible at every width (tests, narrow hosts). */
  alwaysShowActions?: boolean | undefined;
}

/**
 * The progress marker, with the marker's kind on a small test handle so a
 * reader never has to parse the svg: a check once everything is done, an
 * alert for a finished-status module below full completion, otherwise the
 * floored percentage.
 */
function RowProgress({ module }: { module: ModuleRow }): React.ReactElement {
  const marker = moduleProgressMarker(module);
  const percent = modulePercentFloor(module);
  const common = {
    "data-testid": "modules-row-progress",
    "data-progress-kind": marker.kind,
    className: "shrink-0",
  };
  if (marker.kind === "check") {
    return (
      <span {...common}>
        <ProgressRing percent={100} label="All work items done">
          <Check size={12} strokeWidth={2.5} className="text-(--success)" aria-hidden="true" />
        </ProgressRing>
      </span>
    );
  }
  if (marker.kind === "alert") {
    return (
      <span {...common}>
        <ProgressRing percent={percent} label={`${percent}% complete while finished`}>
          <AlertCircle size={12} strokeWidth={2.5} className="text-(--warning)" aria-hidden="true" />
        </ProgressRing>
      </span>
    );
  }
  return (
    <span {...common} data-progress-percent={marker.percent}>
      <ProgressRing percent={marker.percent}>
        <span className="text-caption text-(--text-muted)">{marker.percent}%</span>
      </ProgressRing>
    </span>
  );
}

export function ModuleListRow({
  workspaceSlug,
  projectId,
  module,
  leading,
  trailing,
  archived = false,
  onOpenPeek,
  menu,
  alwaysShowActions = false,
}: ModuleListRowProps): React.ReactElement {
  const hoverClass = alwaysShowActions
    ? ""
    : "opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100 max-md:opacity-100";

  const openPeek = (event: React.MouseEvent): void => {
    event.preventDefault();
    event.stopPropagation();
    onOpenPeek?.(module.id);
  };

  const onRowClick = (event: React.MouseEvent<HTMLElement>): void => {
    if (!archived || onOpenPeek === undefined) return;
    // A press on a control keeps that control's own meaning; only the row's
    // own surface opens the archived peek.
    if (modulePressHitControl(event)) return;
    onOpenPeek(module.id);
  };

  // An archived module has no live detail page, so its name anchor opens
  // the archived peek instead of following the href — and stands the
  // navigation down even when the host supplied no writer. It stays a real
  // anchor so the address still reads as a module link and can be copied.
  const onNameClick = (event: React.MouseEvent<HTMLAnchorElement>): void => {
    if (archived) openPeek(event);
  };

  return (
    <li
      data-testid="modules-row"
      data-module-id={module.id}
      data-module-name={module.name}
      data-module-status={module.status}
      data-module-archived={archived ? "true" : "false"}
      onClick={archived ? onRowClick : undefined}
      className="group flex min-h-12 items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-2)"
    >
      <RowProgress module={module} />
      {leading}
      <Link
        to={moduleDetailHref(workspaceSlug, projectId, module.id)}
        data-testid="modules-row-link"
        onClick={onNameClick}
        className="flex h-10 min-w-0 flex-1 items-center gap-(--space-3)"
      >
        <span data-testid="modules-row-name" className="text-body min-w-0 flex-1 truncate font-medium text-(--text)">
          {module.name}
        </span>
      </Link>
      <IconButton
        label={`Open preview of ${module.name}`}
        size="small"
        data-testid="modules-row-peek"
        className={`shrink-0 ${hoverClass}`}
        onClick={openPeek}
      >
        <Info size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
      {menu === undefined ? null : (
        <div data-testid="modules-row-menu" className="shrink-0 md:hidden">
          {menu}
        </div>
      )}
      {trailing}
    </li>
  );
}
