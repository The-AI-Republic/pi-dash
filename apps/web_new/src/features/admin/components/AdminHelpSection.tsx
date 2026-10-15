// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar help footer (ADM-067, from parity/specs/admin.md §8.6): the
// main-app redirect through the F-09 coexistence helper, the help
// popover with external documentation, forum, and bug-report links
// plus the running instance version, and the collapse toggle.
// Collapsed, the controls restack vertically with hover labels;
// expanded, the redirect carries its own text and no tooltip.
import { BookOpen, Bug, CircleHelp, ExternalLink, MessagesSquare, PanelLeftClose, PanelLeftOpen } from "lucide-react";
import * as React from "react";

import { IconButton, Popover, Tooltip } from "@pidash/kit";

import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { useInstanceBootstrap } from "../hooks.js";
import { useAdminSidebarStore } from "../stores.js";

interface AdminHelpLinkDef {
  name: string;
  href: string;
  Icon: typeof BookOpen;
}

/** Popover links: names and targets pinned by the parity scenario. */
const ADMIN_HELP_LINKS: readonly AdminHelpLinkDef[] = [
  { name: "Documentation", href: "https://github.com/The-AI-Republic/pi-dash/#readme", Icon: BookOpen },
  { name: "Join our Forum", href: "https://github.com/The-AI-Republic/pi-dash/discussions/", Icon: MessagesSquare },
  { name: "Report a bug", href: "https://github.com/The-AI-Republic/pi-dash/issues/new/choose/", Icon: Bug },
];

export function AdminHelpSection(): React.ReactElement {
  const collapsed = useAdminSidebarStore((state) => state.collapsed);
  const toggleCollapsed = useAdminSidebarStore((state) => state.toggleCollapsed);
  const bootstrap = useInstanceBootstrap();
  const version = bootstrap.data?.instance.current_version ?? "";
  const redirect = (
    <CrossAppLink
      to="/"
      data-testid="admin-help-redirect"
      className="text-caption flex items-center gap-(--space-1) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) whitespace-nowrap text-(--text-muted) hover:text-(--text)"
    >
      <ExternalLink size={14} aria-hidden="true" />
      {collapsed ? null : "Redirect to Pi Dash"}
    </CrossAppLink>
  );
  return (
    <div
      data-testid="admin-help-footer"
      className={
        collapsed
          ? "flex flex-col items-center gap-(--space-1) border-t border-(--border) py-(--space-2)"
          : "flex items-center justify-between gap-(--space-1) border-t border-(--border) p-(--space-3)"
      }
    >
      {collapsed ? (
        <Tooltip content="Redirect to Pi Dash">
          <span className="flex items-center">{redirect}</span>
        </Tooltip>
      ) : (
        redirect
      )}
      <Tooltip content="Help">
        <span className="flex items-center">
          <Popover
            trigger={
              <IconButton label="Help" size="small" data-testid="admin-help-trigger">
                <CircleHelp size={16} aria-hidden="true" />
              </IconButton>
            }
          >
            <div className="flex flex-col gap-(--space-1)">
              {ADMIN_HELP_LINKS.map((link) => (
                <a
                  key={link.name}
                  href={link.href}
                  target="_blank"
                  rel="noreferrer"
                  className="text-body flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
                >
                  <link.Icon size={14} aria-hidden="true" className="shrink-0" />
                  {link.name}
                </a>
              ))}
            </div>
            <p
              data-testid="admin-help-version"
              className="text-caption mt-(--space-2) px-(--space-2) text-(--text-muted)"
            >
              Version: v{version}
            </p>
          </Popover>
        </span>
      </Tooltip>
      <Tooltip content="Toggle sidebar">
        <span className="flex items-center">
          <IconButton label="Toggle sidebar" size="small" data-testid="admin-sidebar-toggle" onClick={toggleCollapsed}>
            {collapsed ? (
              <PanelLeftOpen size={16} aria-hidden="true" />
            ) : (
              <PanelLeftClose size={16} aria-hidden="true" />
            )}
          </IconButton>
        </span>
      </Tooltip>
    </div>
  );
}
