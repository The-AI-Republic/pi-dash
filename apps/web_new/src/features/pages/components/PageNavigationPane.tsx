// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Navigation pane shell (parity/specs/pages.md §4.5, §6.5, §6.9,
// PAGE-045/080; the tab bodies belong to NEWFRONT-843). This slice mounts
// the fixed-width pane, its three tabs and their URL reads/writes, and the
// version timeline rows the deep-link protocol points at. The outline,
// assets and statistics bodies arrive with the pane slice.

import { IconButton } from "@pidash/kit";
import { Link } from "@tanstack/react-router";
import { X } from "lucide-react";
import * as React from "react";

import type { PageVersion } from "@pidash/api-client";
import {
  PAGE_PANE_TABS,
  pageDetailDeepHref,
  paneTabLabel,
  type PageDetailSearch,
  type PagePaneTab,
} from "../lib/pageDetail.js";

export const PAGE_PANE_CLOSE = "Close navigation pane";
export const PAGE_CURRENT_VERSION_LABEL = "Current version";

export interface PageNavigationPaneProps {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  search: PageDetailSearch;
  /** Version history, newest first; undefined while the read is in flight. */
  versions: readonly PageVersion[] | undefined;
  onSelectTab: (tab: PagePaneTab) => void;
  onClose: () => void;
  onSelectVersion: (versionId: string) => void;
}

function versionTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

export function PageNavigationPane({
  workspaceSlug,
  projectId,
  pageId,
  search,
  versions,
  onSelectTab,
  onClose,
  onSelectVersion,
}: PageNavigationPaneProps): React.ReactElement {
  const activeLabel = paneTabLabel(search.paneTab);
  return (
    <aside
      aria-label="Navigation pane"
      data-testid="page-navigation-pane"
      className="flex h-full w-80 shrink-0 flex-col border-l border-(--border) bg-(--bg)"
    >
      <div className="flex items-center justify-between border-b border-(--border) px-(--space-4) py-(--space-2)">
        <div role="tablist" aria-label="Page navigation" className="flex items-center gap-(--space-1)">
          {PAGE_PANE_TABS.map((tab) => {
            const active = tab.value === search.paneTab;
            return (
              <button
                key={tab.value}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => onSelectTab(tab.value)}
                className={
                  active
                    ? "text-body rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) font-medium text-(--text)"
                    : "text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
                }
              >
                {tab.label}
              </button>
            );
          })}
        </div>
        <IconButton label={PAGE_PANE_CLOSE} size="small" onClick={onClose}>
          <X size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-(--space-4)">
        {activeLabel === null ? null : (
          <p className="text-caption mb-(--space-4) tracking-wide text-(--text-muted) uppercase">{activeLabel}</p>
        )}
        {search.paneTab === "info" ? (
          <section aria-label="Version history" className="flex flex-col gap-(--space-1)">
            <p className="text-body flex items-center justify-between rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) text-(--text)">
              {PAGE_CURRENT_VERSION_LABEL}
            </p>
            {versions === undefined
              ? [0, 1, 2].map((index) => (
                  <span
                    key={index}
                    role="status"
                    aria-label="Loading versions"
                    className="block h-8 animate-pulse rounded-(--radius-control) bg-(--subtle)"
                  />
                ))
              : versions.map((version) => {
                  const href = pageDetailDeepHref(workspaceSlug, projectId, pageId, {
                    paneTab: "info",
                    version: version.id,
                  });
                  const active = version.id === search.version;
                  return (
                    <Link
                      key={version.id}
                      to={href}
                      data-version-id={version.id}
                      aria-current={active ? "true" : undefined}
                      onClick={(event) => {
                        event.preventDefault();
                        onSelectVersion(version.id);
                      }}
                      className={
                        active
                          ? "text-body flex items-center justify-between rounded-(--radius-control) bg-(--accent-subtle) px-(--space-2) py-(--space-1) text-(--text)"
                          : "text-body flex items-center justify-between rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                      }
                    >
                      <span>{versionTime(version.last_saved_at)}</span>
                    </Link>
                  );
                })}
          </section>
        ) : null}
      </div>
    </aside>
  );
}
