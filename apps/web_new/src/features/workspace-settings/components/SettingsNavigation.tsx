// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Grouped settings navigation (parity/specs/workspace-settings.md §1,
// WSET-001). One list feeds the wide sidebar and the narrow drawer, so
// both always offer the same role-filtered entries in the same order.

import * as React from "react";

import type { VisibleWorkspaceSettingsGroup } from "../lib/sections.js";

export interface SettingsNavigationProps {
  groups: readonly VisibleWorkspaceSettingsGroup[];
  /** Section key of the current address. */
  activeSection: string;
  /** In-app navigation; plain href receives an ordinary browser path. */
  onNavigate: (href: string) => void;
  /** Optional per-link hook, used by the drawer to close after a pick. */
  onFollow?: (() => void) | undefined;
  /** DOM id for the group heading association. */
  idPrefix: string;
}

export function SettingsNavigation({
  groups,
  activeSection,
  onNavigate,
  onFollow,
  idPrefix,
}: SettingsNavigationProps): React.ReactElement {
  return (
    <nav aria-label="Workspace settings" data-testid="workspace-settings-nav">
      {groups.map((group) => {
        const headingId = `${idPrefix}-${group.key}`;
        return (
          <section key={group.key} aria-labelledby={headingId} className="py-(--space-2)">
            <h2
              id={headingId}
              className="text-caption px-(--space-3) py-(--space-1) font-medium tracking-wide text-(--text-muted) uppercase"
            >
              {group.label}
            </h2>
            <ul className="flex flex-col gap-px">
              {group.sections.map((section) => {
                const active = section.key === activeSection;
                return (
                  <li key={section.key}>
                    <a
                      href={section.href}
                      aria-current={active ? "page" : undefined}
                      onClick={(event) => {
                        // Modified clicks keep their browser meaning.
                        if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
                        event.preventDefault();
                        onFollow?.();
                        onNavigate(section.href);
                      }}
                      className={`text-body flex items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) ${
                        active
                          ? "bg-(--accent-subtle) font-medium text-(--text)"
                          : "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                      }`}
                    >
                      {section.label}
                    </a>
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
    </nav>
  );
}
