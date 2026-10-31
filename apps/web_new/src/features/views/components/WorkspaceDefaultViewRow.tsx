// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One static default row of the workspace views list (parity/specs/views.md
// §9, VIEW-030/031). A bare link and nothing else: no star, no overflow
// menu, no access badge and no owner — the default has no record to act on.
// Open-in-new-tab and copy-link live on the default's detail header, not
// here.

import { Link } from "@tanstack/react-router";
import { List } from "lucide-react";
import * as React from "react";

import { workspaceDefaultViewHref, type WorkspaceDefaultView } from "../lib/workspaceViews.js";

export interface WorkspaceDefaultViewRowProps {
  workspaceSlug: string;
  view: WorkspaceDefaultView;
}

export function WorkspaceDefaultViewRow({ workspaceSlug, view }: WorkspaceDefaultViewRowProps): React.ReactElement {
  return (
    <li data-testid="workspace-views-default-row" data-view-key={view.key} className="border-b border-(--border)">
      <Link
        to={workspaceDefaultViewHref(workspaceSlug, view.key)}
        className="flex h-11 items-center gap-(--space-3) px-(--space-4)"
      >
        <List size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
        <span className="text-body truncate font-medium text-(--text)">{view.label}</span>
      </Link>
    </li>
  );
}
