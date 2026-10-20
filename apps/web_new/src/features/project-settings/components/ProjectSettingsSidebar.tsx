// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings sidebar (PSET-001, PSET-004; parity/specs/project-settings-shell.md).
// A grouped, role-filtered rail: a header with the way back to the issue
// board, the project mark and the viewer's role, then one group per
// surviving tab group. Entries link to slash-terminated addresses and the
// canonical form carries the selected marker; the equivalent no-slash
// address renders the same tab with nothing marked (bug NEWFRONT-396).

import { Link, useLinkProps, type UseLinkPropsOptions } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import * as React from "react";

import { getAppBaseUrl } from "../../../core/api/client.js";
import { ProjectLogo } from "../../projects/index.js";
import { isTabActive, projectSettingsTabHref, selectSettingsNav, type ProjectRole } from "../tabs.js";

export const ROLE_LABELS: Record<number, string> = {
  5: "Guest",
  15: "Member",
  20: "Admin",
};

export function roleLabel(role: ProjectRole): string {
  if (role === null) return "";
  return ROLE_LABELS[role] ?? "Member";
}

const SELECTED_CLASS = "bg-layer-transparent-selected";
const IDLE_CLASS = "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)";

type RouterLinkProps = Parameters<typeof Link>[0];

function hrefLinkProps(href: string): RouterLinkProps {
  return { to: href } as RouterLinkProps;
}

/**
 * A rail entry that owns its current-page marker. The router sets
 * `aria-current` for every fuzzy ancestor match, which would mark General
 * on each sub-tab; building the link through `useLinkProps` keeps
 * navigation and preload semantics while this component decides the
 * single current entry from the slash-sensitive `active` flag.
 */
function RailLink({
  href,
  active,
  testId,
  children,
}: {
  href: string;
  active: boolean;
  testId: string;
  children: React.ReactNode;
}): React.ReactElement {
  const linkProps = useLinkProps({ to: href } as UseLinkPropsOptions);
  return (
    <a
      {...(linkProps as React.AnchorHTMLAttributes<HTMLAnchorElement>)}
      aria-current={active ? "page" : undefined}
      data-testid={testId}
      data-active={active ? "true" : "false"}
      className={`text-body flex w-full items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) ${active ? SELECTED_CLASS : IDLE_CLASS}`}
    >
      {children}
    </a>
  );
}

export interface ProjectSettingsSidebarProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  logoProps: Record<string, unknown>;
  identifier: string;
  role: ProjectRole;
  /** Pathname the active marker compares against. */
  pathname: string;
  /** Extra classes applied to the panel root (drawer sizing). */
  className?: string;
}

export function ProjectSettingsSidebar({
  workspaceSlug,
  projectId,
  projectName,
  logoProps,
  identifier,
  role,
  pathname,
  className,
}: ProjectSettingsSidebarProps): React.ReactElement | null {
  const groups = selectSettingsNav(role);
  if (role === null) return null;
  return (
    <aside
      aria-label="Project settings navigation"
      data-testid="project-settings-sidebar"
      className={`flex h-full w-64 shrink-0 flex-col overflow-y-auto border-r border-(--border) bg-(--surface) ${className ?? ""}`}
    >
      <div className="flex shrink-0 flex-col gap-(--space-2) border-b border-(--border) px-(--space-4) py-(--space-3)">
        <div className="flex items-center gap-(--space-2)">
          <Link
            {...hrefLinkProps(`/${workspaceSlug}/projects/${projectId}/issues/`)}
            aria-label="Back to project"
            className="flex size-7 items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          >
            <ArrowLeft size={16} aria-hidden="true" />
          </Link>
          <span className="text-caption text-(--text-muted)">Project settings</span>
        </div>
        <div className="flex min-w-0 items-center gap-(--space-2)">
          <ProjectLogo logoProps={logoProps} identifier={identifier} apiBaseUrl={getAppBaseUrl()} />
          <div className="flex min-w-0 flex-col">
            <span className="text-body truncate font-medium text-(--text)">{projectName}</span>
            <span className="text-caption truncate text-(--text-muted)">{roleLabel(role)}</span>
          </div>
        </div>
      </div>
      <nav aria-label="Settings tabs" className="flex flex-col gap-(--space-4) px-(--space-3) py-(--space-3)">
        {groups.map((group) => (
          <div key={group.key} data-testid={`project-settings-group-${group.key}`}>
            <div
              data-testid={`project-settings-group-label-${group.key}`}
              className="text-caption px-(--space-3) pb-(--space-1) font-medium text-(--text-muted)"
            >
              {group.label}
            </div>
            <ul className="flex flex-col gap-px">
              {group.tabs.map((tab) => {
                const href = projectSettingsTabHref(workspaceSlug, projectId, tab);
                const active = isTabActive(pathname, workspaceSlug, projectId, tab);
                return (
                  <li key={tab.key}>
                    <RailLink href={href} active={active} testId={`project-settings-tab-${tab.key}`}>
                      {tab.label}
                    </RailLink>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </nav>
    </aside>
  );
}
