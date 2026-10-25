// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Navigation pane shell (parity/specs/pages.md §6.5, §6.9, PAGE-045/080;
// tab bodies PAGE-046–050). A right-hand pane that stays mounted and slides
// away by its own width, so the canvas reflows with it on a transition.
// It carries the three tabs in their pinned order and whatever the active
// one shows: the outline, the live statistics with attribution, or the
// assets. While it is shut, the heading preview sits at the canvas edge in
// its place. An edition may swap the whole pane for its own panels.

import { IconButton } from "@pidash/kit";
import { Link } from "@tanstack/react-router";
import { X } from "lucide-react";
import * as React from "react";

import type { PageVersion } from "@pidash/api-client";
import { edition } from "../../../core/edition/index.js";
import { PAGE_PANE_TABS, pageDetailDeepHref, type PageDetailSearch, type PagePaneTab } from "../lib/pageDetail.js";
import {
  PAGE_PANE_WIDTH_PX,
  type PageActorRow,
  type PageAssetRow,
  type PageDocument,
  type PageHeading,
} from "../lib/pagePane.js";
import { PageActorsInfo } from "./PageActorsInfo.js";
import { PageAssetsPanel } from "./PageAssetsPanel.js";
import { PageDocumentStats } from "./PageDocumentStats.js";
import { PageOutlinePanel } from "./PageOutlinePanel.js";

export const PAGE_PANE_CLOSE = "Close navigation pane";
export const PAGE_CURRENT_VERSION_LABEL = "Current version";
export const PAGE_NAVIGATION_LABEL = "Navigation pane";

export interface PageNavigationPaneProps {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  search: PageDetailSearch;
  /** The live document the tabs read (PAGE-046–049). */
  document: PageDocument;
  /** Attribution rows for the info tab (PAGE-050). */
  actors: readonly PageActorRow[];
  /** Version history, newest first; undefined while the read is in flight. */
  versions: readonly PageVersion[] | undefined;
  onSelectTab: (tab: PagePaneTab) => void;
  onClose: () => void;
  onSelectVersion: (versionId: string) => void;
  /** Bring one outline heading into view (PAGE-047). */
  onSelectHeading: (heading: PageHeading) => void;
  /** Edition renderer for attachment kinds beyond images (§6.7). */
  renderAttachment?: ((asset: PageAssetRow) => React.ReactNode) | undefined;
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

function PaneTabs({
  active,
  onSelect,
}: {
  active: PagePaneTab | undefined;
  onSelect: (tab: PagePaneTab) => void;
}): React.ReactElement {
  return (
    <div role="tablist" aria-label="Page navigation" className="flex items-center gap-(--space-1)">
      {PAGE_PANE_TABS.map((tab) => {
        const selected = tab.value === active;
        return (
          <button
            key={tab.value}
            type="button"
            role="tab"
            aria-selected={selected}
            onClick={() => onSelect(tab.value)}
            className={
              selected
                ? "text-body cursor-pointer rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) font-medium text-(--text)"
                : "text-body cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            }
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}

interface VersionTimelineProps {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  search: PageDetailSearch;
  versions: readonly PageVersion[] | undefined;
  onSelectVersion: (versionId: string) => void;
}

function VersionTimeline({
  workspaceSlug,
  projectId,
  pageId,
  search,
  versions,
  onSelectVersion,
}: VersionTimelineProps): React.ReactElement {
  return (
    <section aria-label="Version history" className="flex flex-col gap-(--space-1)">
      <h3 className="text-caption mb-(--space-2) tracking-wide text-(--text-muted) uppercase">Version history</h3>
      <ul className="flex flex-col gap-(--space-1)">
        <li className="min-w-0">
          <p className="text-body flex items-center justify-between rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) text-(--text)">
            {PAGE_CURRENT_VERSION_LABEL}
          </p>
        </li>
        {versions === undefined
          ? [0, 1, 2].map((index) => (
              <li key={index} className="min-w-0">
                <span
                  role="status"
                  aria-label="Loading versions"
                  className="block h-8 animate-pulse rounded-(--radius-control) bg-(--subtle)"
                />
              </li>
            ))
          : versions.map((version) => {
              const href = pageDetailDeepHref(workspaceSlug, projectId, pageId, {
                paneTab: "info",
                version: version.id,
              });
              const active = version.id === search.version;
              return (
                <li key={version.id} className="min-w-0">
                  <Link
                    to={href}
                    data-version-id={version.id}
                    aria-current={active ? "true" : undefined}
                    onClick={(event) => {
                      event.preventDefault();
                      onSelectVersion(version.id);
                    }}
                    className={
                      active
                        ? "text-body flex items-center justify-between rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) text-(--text)"
                        : "text-body flex items-center justify-between rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                    }
                  >
                    <p className="min-w-0 truncate">{versionTime(version.last_saved_at)}</p>
                  </Link>
                </li>
              );
            })}
      </ul>
    </section>
  );
}

interface PaneBodyProps {
  tab: PagePaneTab;
  document: PageDocument;
  actors: readonly PageActorRow[];
  onSelectHeading: (heading: PageHeading) => void;
  renderAttachment?: ((asset: PageAssetRow) => React.ReactNode) | undefined;
}

function PaneBody({ tab, document, actors, onSelectHeading, renderAttachment }: PaneBodyProps): React.ReactElement {
  if (tab === "outline") return <PageOutlinePanel headings={document.headings} onSelectHeading={onSelectHeading} />;
  if (tab === "assets") {
    return <PageAssetsPanel assets={document.assets} renderAttachment={renderAttachment} />;
  }
  return (
    <div className="flex flex-col gap-(--space-6)">
      <PageDocumentStats stats={document.stats} />
      <PageActorsInfo rows={actors} />
    </div>
  );
}

export function PageNavigationPane(props: PageNavigationPaneProps): React.ReactElement {
  const {
    workspaceSlug,
    projectId,
    pageId,
    search,
    document,
    actors,
    versions,
    onSelectTab,
    onClose,
    onSelectVersion,
  } = props;
  const open = search.paneTab !== undefined;
  // An edition's replacement body takes over the tab area only; the shell,
  // the tabs and the close control stay the app's (PAGE-045, §9.4).
  const EditionBody = edition.slots?.pageNavigationPane;
  return (
    <aside
      aria-label={PAGE_NAVIGATION_LABEL}
      data-testid="page-navigation-pane"
      data-open={open ? "true" : "false"}
      style={{
        width: `${PAGE_PANE_WIDTH_PX}px`,
        // Sliding out by the pane's own width keeps it out of the layout's
        // way while it is shut; the transition makes the canvas reflow.
        marginRight: open ? "0px" : `-${PAGE_PANE_WIDTH_PX}px`,
      }}
      className="flex h-full shrink-0 flex-col overflow-hidden border-l border-(--border) bg-(--bg) transition-[margin] duration-(--motion-duration) ease-(--motion-easing)"
    >
      <div className="flex items-center justify-between border-b border-(--border) px-(--space-4) py-(--space-2)">
        {open ? <PaneTabs active={search.paneTab} onSelect={onSelectTab} /> : null}
        <span className="flex-1" aria-hidden="true" />
        <IconButton label={PAGE_PANE_CLOSE} size="small" onClick={onClose}>
          <X size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
      {search.paneTab === undefined ? null : (
        <div className="min-h-0 flex-1 overflow-y-auto p-(--space-4)">
          {EditionBody === undefined ? (
            <PaneBody
              tab={search.paneTab}
              document={document}
              actors={actors}
              onSelectHeading={props.onSelectHeading}
              renderAttachment={props.renderAttachment}
            />
          ) : (
            <EditionBody tab={search.paneTab} workspaceSlug={workspaceSlug} projectId={projectId} pageId={pageId} />
          )}
          {search.paneTab === "info" ? (
            <div className="mt-(--space-6)">
              <VersionTimeline
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                pageId={pageId}
                search={search}
                versions={versions}
                onSelectVersion={onSelectVersion}
              />
            </div>
          ) : null}
        </div>
      )}
    </aside>
  );
}
