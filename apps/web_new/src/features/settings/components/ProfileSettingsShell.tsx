// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Full-page personal-settings shell (parity/specs/profile.md §5,
// PROF-009/010). One sidebar carries the back control, the title, the two
// grouped tab lists and the workspace shortcuts; the tab body renders in
// the content pane. Tab switches are link navigations to the tab
// addresses; there is no dialog variant anywhere (the main#573 target).
// Importing the built-in registrations here keeps the general, security
// and notifications bodies in this shell's (lazy) route chunk rather
// than the entry bundle, so a direct tab visit always finds its body.

import { ArrowLeft } from "lucide-react";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/document.js";
import "../pageEntries.js";
import type { Workspace } from "@pidash/api-client";
import { profileSettingsHref, type ProfileSettingsTabId } from "../lib/tabNavigation.js";
import { isProfileSettingsTabServed } from "../lib/visibility.js";
import { profileSettingsGroups, profileSettingsTitle } from "../lib/tabs.js";

export interface ProfileSettingsShellProps {
  activeTab: ProfileSettingsTabId;
  /**
   * The instance's self-managed marker (CLOUD-035). A hosted instance
   * hides the password tab; unknown keeps the full list.
   */
  selfManaged?: boolean | undefined;
  /** Workspaces the shortcuts list offers, in the caller's order. */
  workspaces: readonly Pick<Workspace, "id" | "slug" | "name">[];
  /** Client-side navigation to a plain href. */
  onNavigate: (href: string) => void;
  /** The tab body. */
  children: React.ReactNode;
}

export const BACK_HOME_LABEL = "Back to home";

export const SETTINGS_TITLE = "Profile settings";

export function ProfileSettingsShell({
  activeTab,
  selfManaged,
  workspaces,
  onNavigate,
  children,
}: ProfileSettingsShellProps): React.ReactElement {
  useDocumentHead({ title: profileSettingsTitle(activeTab), robots: "noindex, nofollow" });
  const groups = profileSettingsGroups((tab) => isProfileSettingsTabServed(tab, selfManaged));

  const follow = (event: React.MouseEvent<HTMLAnchorElement>, href: string, external = false): void => {
    // Modified clicks keep their browser meaning (new tab, download, …).
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    if (external) {
      window.location.assign(href);
      return;
    }
    onNavigate(href);
  };

  return (
    <div data-testid="profile-settings-shell" className="flex min-h-screen bg-(--bg) text-(--text)">
      <aside
        aria-label="Profile settings"
        data-testid="profile-settings-sidebar"
        className="flex w-64 shrink-0 flex-col gap-(--space-2) border-r border-(--border) bg-(--surface) p-(--space-3)"
      >
        <div className="flex items-center gap-(--space-2)">
          <a
            href="/"
            aria-label={BACK_HOME_LABEL}
            className="inline-flex size-8 items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            onClick={(event) => follow(event, "/", true)}
          >
            <ArrowLeft size={16} strokeWidth={1.5} aria-hidden="true" />
          </a>
          <span className="text-body font-medium text-(--text)">{SETTINGS_TITLE}</span>
        </div>

        <nav className="flex flex-col gap-(--space-4)">
          {groups.map((group) => (
            <section key={group.id} aria-labelledby={`profile-settings-${group.id}`}>
              <h2
                id={`profile-settings-${group.id}`}
                className="text-caption px-(--space-2) py-(--space-1) font-medium tracking-wide text-(--text-muted) uppercase"
              >
                {group.label}
              </h2>
              <ul className="flex flex-col gap-px">
                {group.tabs.map((tab) => {
                  const active = tab.id === activeTab;
                  return (
                    <li key={tab.id}>
                      <a
                        href={profileSettingsHref(tab.id)}
                        aria-current={active ? "page" : undefined}
                        data-testid={`profile-settings-tab-${tab.id}`}
                        className={`text-body flex items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) ${
                          active
                            ? "bg-(--accent-subtle) font-medium text-(--text)"
                            : "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                        }`}
                        onClick={(event) => follow(event, profileSettingsHref(tab.id))}
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

        <section aria-labelledby="profile-settings-workspace" className="mt-auto">
          <h2
            id="profile-settings-workspace"
            className="text-caption px-(--space-2) py-(--space-1) font-medium tracking-wide text-(--text-muted) uppercase"
          >
            Workspace
          </h2>
          <ul className="flex flex-col gap-px">
            {workspaces.map((workspace) => (
              <li key={workspace.id}>
                <a
                  href={`/${workspace.slug}/`}
                  className="text-body flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                  onClick={(event) => follow(event, `/${workspace.slug}/`)}
                >
                  <span
                    aria-hidden="true"
                    className="text-caption grid size-6 shrink-0 place-items-center rounded-(--radius-control) bg-(--subtle) font-medium text-(--text)"
                  >
                    {workspaceInitial(workspace.name)}
                  </span>
                  <span className="truncate">{workspace.name}</span>
                </a>
              </li>
            ))}
            <li>
              <a
                href="/create-workspace/"
                className="text-body flex items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                onClick={(event) => follow(event, "/create-workspace/")}
              >
                Create workspace
              </a>
            </li>
            <li>
              <a
                href="/invitations/"
                className="text-body flex items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                onClick={(event) => follow(event, "/invitations/")}
              >
                Workspace invites
              </a>
            </li>
          </ul>
        </section>
      </aside>

      <main data-testid="profile-settings-content" className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto w-full max-w-4xl px-(--space-8) py-(--space-8)">{children}</div>
      </main>
    </div>
  );
}

/** The character shown ahead of a workspace name in the shortcut list. */
export function workspaceInitial(name: string): string {
  const first = name.trim().charAt(0);
  return first === "" ? "?" : first.toUpperCase();
}
