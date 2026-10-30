// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One workspace in the account navigator (parity/specs/profile.md §5;
// main#675). The row expands in place to that workspace's permitted
// settings pages, drawn from the same catalog the standalone
// `/<ws>/settings*` navigator renders. A workspace where the viewer may
// open nothing has no expander at all and shrinks to a plain label.

import { ChevronRight } from "lucide-react";
import * as React from "react";

import { selectPermissions } from "../../../../core/session/index.js";
import {
  isWorkspaceSettingLinkActive,
  permittedWorkspaceSettingGroups,
  type WorkspaceSettingLink,
} from "../../../workspace-settings/index.js";

export interface WorkspaceSettingsRowProps {
  workspace: { id: string; slug: string; name: string; role: number };
  /** Whether the group opens on mount (the workspace the viewer is in). */
  initiallyExpanded: boolean;
  /** Address the highlight rules read (main#675). */
  pathname: string;
  /** In-app navigation to a plain href. */
  onNavigate: (href: string) => void;
  /** Distinguishes the docked copy's ids from the drawer copy's. */
  idPrefix?: string;
}

/** The character shown ahead of a workspace name. */
export function workspaceInitial(name: string): string {
  const first = name.trim().charAt(0);
  return first === "" ? "?" : first.toUpperCase();
}

function WorkspaceMark({ name }: { name: string }): React.ReactElement {
  return (
    <span
      aria-hidden="true"
      className="text-caption grid size-6 shrink-0 place-items-center rounded-(--radius-control) bg-(--subtle) font-medium text-(--text)"
    >
      {workspaceInitial(name)}
    </span>
  );
}

export function WorkspaceSettingsRow({
  workspace,
  initiallyExpanded,
  pathname,
  onNavigate,
  idPrefix = "account-settings",
}: WorkspaceSettingsRowProps): React.ReactElement {
  const groups = permittedWorkspaceSettingGroups(workspace.slug, selectPermissions(workspace.role));
  const [expanded, setExpanded] = React.useState(initiallyExpanded);
  const groupId = `${idPrefix}-workspace-${workspace.id}`;

  const follow = (event: React.MouseEvent<HTMLAnchorElement>, href: string): void => {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    onNavigate(href);
  };

  if (groups.length === 0) {
    // Nothing openable means no expander at all: the row keeps the way
    // home it had before the nesting arrived, minus the chevron.
    return (
      <li data-testid={`account-settings-workspace-${workspace.slug}`} data-expandable="false">
        <a
          href={`/${workspace.slug}/`}
          data-testid={`account-settings-workspace-home-${workspace.slug}`}
          onClick={(event) => {
            follow(event, `/${workspace.slug}/`);
          }}
          className="text-body flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <WorkspaceMark name={workspace.name} />
          <span className="truncate">{workspace.name}</span>
        </a>
      </li>
    );
  }

  return (
    <li data-testid={`account-settings-workspace-${workspace.slug}`} data-expandable="true">
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={groupId}
        data-testid={`account-settings-workspace-toggle-${workspace.slug}`}
        onClick={() => {
          setExpanded((current) => !current);
        }}
        className="text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--subtle)"
      >
        <WorkspaceMark name={workspace.name} />
        <span className="min-w-0 flex-1 truncate">{workspace.name}</span>
        <ChevronRight
          size={14}
          strokeWidth={1.5}
          aria-hidden="true"
          className={`shrink-0 text-(--text-muted) transition-transform ${expanded ? "rotate-90" : ""}`}
        />
      </button>
      {expanded ? (
        <div id={groupId} data-testid={`account-settings-workspace-group-${workspace.slug}`} className="pl-(--space-6)">
          {groups.map((group) => (
            <section key={group.key} aria-label={`${workspace.name} ${group.label}`} className="pb-(--space-1)">
              <h3 className="text-caption px-(--space-2) py-(--space-1) font-medium tracking-wide text-(--text-muted) uppercase">
                {group.label}
              </h3>
              <ul className="flex flex-col gap-px">
                {group.links.map((link) => (
                  <li key={link.key}>
                    <WorkspaceSettingEntry
                      workspaceSlug={workspace.slug}
                      link={link}
                      pathname={pathname}
                      onFollow={follow}
                    />
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      ) : null}
    </li>
  );
}

function WorkspaceSettingEntry({
  workspaceSlug,
  link,
  pathname,
  onFollow,
}: {
  workspaceSlug: string;
  link: WorkspaceSettingLink;
  pathname: string;
  onFollow: (event: React.MouseEvent<HTMLAnchorElement>, href: string) => void;
}): React.ReactElement {
  const active = isWorkspaceSettingLinkActive(pathname, workspaceSlug, link);
  return (
    <a
      href={link.href}
      aria-current={active ? "page" : undefined}
      data-testid={`account-settings-workspace-link-${workspaceSlug}-${link.key}`}
      onClick={(event) => {
        onFollow(event, link.href);
      }}
      className={`text-body flex items-center rounded-(--radius-control) px-(--space-2) py-(--space-1) ${
        active
          ? "bg-(--accent-subtle) font-medium text-(--text)"
          : "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
      }`}
    >
      {link.label}
    </a>
  );
}
