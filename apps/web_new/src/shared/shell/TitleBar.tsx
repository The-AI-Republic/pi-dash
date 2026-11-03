// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Title bar area (Architecture > shared/shell). Brand and workspace crumb
// on the left, the palette search box in the middle, the signed-in user on
// the right. On desktop (platform.window present) the header is also the
// window drag region, indented for the macOS traffic lights, and it mirrors
// the location in the native window title. The web build renders plain
// chrome: no drag region, no inset, no title sync.

import { Avatar } from "@pidash/kit";
import * as React from "react";

import { platform } from "../../core/platform/index.js";
import type { PaletteSearchScope, SearchResultHit } from "../commands/index.js";
import { RepoStar } from "./RepoStar.js";
import { TopBarSearch } from "./TopBarSearch.js";
import { useWindowTitle } from "./windowTitle.js";

export interface TitleBarProps {
  workspaceName: string;
  section: string;
  userLabel: string;
  /** Where the search box searches; null renders it commands-only. */
  searchScope: PaletteSearchScope | null;
  /** Runs when a server hit activates (route-owned navigation). */
  onSelectSearchResult: (hit: SearchResultHit) => void;
}

export function TitleBar({
  workspaceName,
  section,
  userLabel,
  searchScope,
  onSelectSearchResult,
}: TitleBarProps): React.ReactElement {
  const desktop = platform.window;
  const title = section ? `${section} — Pi Dash` : "Pi Dash";

  useWindowTitle(title);

  return (
    <header
      {...(desktop ? { "data-tauri-drag-region": true } : {})}
      className={`flex h-12 shrink-0 items-center gap-(--space-4) border-b border-(--border) bg-(--bg) px-(--space-4)${
        desktop ? " select-none" : ""
      }${desktop?.trafficLightInset ? " pl-20" : ""}`}
    >
      <div className="flex min-w-0 items-center gap-(--space-2)" aria-label="Location">
        <span className="text-emphasis text-(--text)">Pi Dash</span>
        <span aria-hidden="true" className="text-(--text-muted)">
          /
        </span>
        <span className="text-body truncate text-(--text-muted)">{workspaceName}</span>
        {section ? (
          <>
            <span aria-hidden="true" className="text-(--text-muted)">
              /
            </span>
            <span className="text-body truncate text-(--text)">{section}</span>
          </>
        ) : null}
      </div>
      <div className="flex flex-1 justify-center">
        <TopBarSearch scope={searchScope} onSelectSearchResult={onSelectSearchResult} />
      </div>
      <div className="flex items-center gap-(--space-3)">
        <RepoStar />
        <Avatar name={userLabel} size="small" />
        <span className="text-body hidden max-w-40 truncate text-(--text-muted) sm:inline">{userLabel}</span>
      </div>
    </header>
  );
}
