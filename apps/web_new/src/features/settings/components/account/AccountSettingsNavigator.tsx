// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account-settings navigator (parity/specs/profile.md §5, PROF-009/010;
// main#670/#675). Three blocks: the back control and title, the two
// grouped tab lists, and the Workspace section whose rows expand in place
// to each workspace's settings pages. The tab is read from the caller's
// address, never held here. Narrow screens render the same content inside
// a drawer opened from a hamburger; wide screens dock it as the shell's
// middle panel.

import { ArrowLeft, Menu } from "lucide-react";
import * as React from "react";

import type { Workspace } from "@pidash/api-client";
import type { ProfileSettingsTabId } from "../../lib/tabNavigation.js";
import { profileSettingsGroups } from "../../lib/tabs.js";
import { WorkspaceSettingsRow } from "./WorkspaceSettingsRow.js";

export const ACCOUNT_SETTINGS_TITLE = "Profile settings";
export const ACCOUNT_SETTINGS_BACK_LABEL = "Back to home";
export const ACCOUNT_SETTINGS_NAV_LABEL = "Profile settings";

export interface AccountSettingsNavigatorProps {
  /** The workspace whose home the back control returns to. */
  workspaceSlug: string;
  /** Tab named by the address. */
  activeTab: ProfileSettingsTabId;
  /** Every workspace the viewer belongs to, in caller order. */
  workspaces: readonly Pick<Workspace, "id" | "slug" | "name" | "role">[];
  /** Labels the current tab; null while the address names none. */
  activeLabel: string | null;
  /**
   * Address the workspace links read for their highlight (main#675). On an
   * account-settings address no workspace entry can match, so nothing in
   * the Workspace section reads current while one of its tabs is open.
   */
  pathname: string;
  /** Client-side navigation to a plain href. */
  onNavigate: (href: string) => void;
  /** Restricts the tab list to served tabs (the cloud password withdrawal). */
  isTabServed: (tab: ProfileSettingsTabId) => boolean;
  /** Builds the tab address. */
  tabHref: (tab: ProfileSettingsTabId) => string;
}

export function AccountSettingsNavigator(props: AccountSettingsNavigatorProps): React.ReactElement {
  const [drawerOpen, setDrawerOpen] = React.useState(false);
  const rootRef = React.useRef<HTMLDivElement | null>(null);
  const triggerRef = React.useRef<HTMLButtonElement | null>(null);

  React.useEffect(() => {
    if (!drawerOpen) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (rootRef.current?.contains(event.target as Node) === true) return;
      setDrawerOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [drawerOpen]);

  // The docked panel and the drawer hold the same list, so each copy
  // carries its own id prefix; sharing one would repeat element ids in the
  // document and point every aria-labelledby at whichever copy came first.
  const content = (
    <AccountSettingsNavigatorContent
      {...props}
      idPrefix="account-settings-panel"
      // The docked copy keeps the plain test ids the parity driver reads;
      // only the drawer copy namespaces its own.
      testIdPrefix="account-settings"
      onFollow={() => {
        setDrawerOpen(false);
      }}
    />
  );

  return (
    <div ref={rootRef} className="contents">
      <aside
        aria-label={ACCOUNT_SETTINGS_NAV_LABEL}
        data-testid="profile-settings-sidebar"
        className="hidden h-full w-64 shrink-0 flex-col overflow-y-auto border-r border-(--border) bg-(--bg) md:flex"
      >
        {content}
      </aside>
      <div className="relative flex shrink-0 flex-col border-b border-(--border) md:hidden">
        <div className="flex items-center gap-(--space-2) px-(--space-3) py-(--space-2)">
          <button
            ref={triggerRef}
            type="button"
            aria-label="Open settings navigation"
            aria-expanded={drawerOpen}
            aria-controls="account-settings-drawer"
            data-testid="account-settings-menu"
            onClick={() => {
              setDrawerOpen((current) => !current);
            }}
            className="inline-flex size-8 items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          >
            <Menu size={18} strokeWidth={1.5} aria-hidden="true" />
          </button>
          <span data-testid="account-settings-mobile-crumb" className="text-body truncate text-(--text)">
            {props.activeLabel ?? ACCOUNT_SETTINGS_TITLE}
          </span>
        </div>
        {drawerOpen ? (
          <div
            id="account-settings-drawer"
            data-testid="account-settings-drawer"
            className="shadow-dialog absolute top-full left-0 z-30 w-72 rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-2) md:hidden"
            onKeyDown={(event) => {
              if (event.key !== "Escape") return;
              event.stopPropagation();
              setDrawerOpen(false);
              triggerRef.current?.focus();
            }}
          >
            <AccountSettingsNavigatorContent
              {...props}
              idPrefix="account-settings-drawer"
              testIdPrefix="account-settings-drawer"
              onFollow={() => {
                setDrawerOpen(false);
              }}
            />
          </div>
        ) : null}
      </div>
    </div>
  );
}

function AccountSettingsNavigatorContent({
  workspaceSlug,
  activeTab,
  workspaces,
  pathname,
  onNavigate,
  isTabServed,
  tabHref,
  idPrefix,
  testIdPrefix,
  onFollow,
}: AccountSettingsNavigatorProps & {
  idPrefix: string;
  testIdPrefix: string;
  onFollow: () => void;
}): React.ReactElement {
  const groups = profileSettingsGroups(isTabServed);

  const follow = (event: React.MouseEvent<HTMLAnchorElement>, href: string): void => {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    onFollow();
    onNavigate(href);
  };

  return (
    <div className="flex flex-col gap-(--space-4) p-(--space-3)">
      <div className="flex items-center gap-(--space-2)">
        <a
          href={`/${workspaceSlug}`}
          aria-label={ACCOUNT_SETTINGS_BACK_LABEL}
          data-testid="profile-settings-back"
          className="inline-flex size-8 items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          onClick={(event) => {
            follow(event, `/${workspaceSlug}`);
          }}
        >
          <ArrowLeft size={16} strokeWidth={1.5} aria-hidden="true" />
        </a>
        <span className="text-body font-medium text-(--text)">{ACCOUNT_SETTINGS_TITLE}</span>
      </div>

      <nav aria-label={ACCOUNT_SETTINGS_NAV_LABEL} className="flex flex-col gap-(--space-3)">
        {groups.map((group) => (
          <section key={group.id} aria-labelledby={`${idPrefix}-group-${group.id}`}>
            <h2
              id={`${idPrefix}-group-${group.id}`}
              className="text-caption px-(--space-2) py-(--space-1) font-medium tracking-wide text-(--text-muted) uppercase"
            >
              {group.label}
            </h2>
            <ul className="flex flex-col gap-px">
              {group.tabs.map((tab) => {
                const active = tab.id === activeTab;
                const href = tabHref(tab.id);
                return (
                  <li key={tab.id}>
                    <a
                      href={href}
                      aria-current={active ? "page" : undefined}
                      data-testid={`profile-settings-tab-${tab.id}`}
                      className={`text-body flex items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) ${
                        active
                          ? "bg-(--accent-subtle) font-medium text-(--text)"
                          : "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                      }`}
                      onClick={(event) => {
                        follow(event, href);
                      }}
                    >
                      {tab.label}
                    </a>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
      </nav>

      <section aria-labelledby={`${idPrefix}-workspace-heading`}>
        <h2
          id={`${idPrefix}-workspace-heading`}
          className="text-caption px-(--space-2) py-(--space-1) font-medium tracking-wide text-(--text-muted) uppercase"
        >
          Workspace
        </h2>
        <ul className="flex flex-col gap-px">
          {workspaces.map((workspace) => (
            // The key carries the viewed flag too: a hop to another
            // workspace's settings remounts the row, so which one starts
            // open always follows the address (a param change alone keeps
            // component state).
            <WorkspaceSettingsRow
              key={`${workspace.id}:${String(workspace.slug === workspaceSlug)}`}
              idPrefix={idPrefix}
              testIdPrefix={testIdPrefix}
              workspace={workspace}
              initiallyExpanded={workspace.slug === workspaceSlug}
              pathname={pathname}
              onNavigate={onNavigate}
            />
          ))}
          <li>
            <a
              href="/create-workspace/"
              className="text-body flex items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
              onClick={(event) => {
                follow(event, "/create-workspace/");
              }}
            >
              Create workspace
            </a>
          </li>
          <li>
            <a
              href="/invitations/"
              className="text-body flex items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
              onClick={(event) => {
                follow(event, "/invitations/");
              }}
            >
              Workspace invites
            </a>
          </li>
        </ul>
      </section>
    </div>
  );
}
