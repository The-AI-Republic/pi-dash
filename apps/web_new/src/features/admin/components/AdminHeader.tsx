// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard header (ADM-066, ADM-068, from parity/specs/admin.md
// §8.5/§8.7): the mobile hamburger plus the breadcrumb trail. Every
// crumb links — including the terminal one — so the trail is
// admin-local rather than the kit Breadcrumbs, which render the last
// entry as text.
import { Link, useLocation } from "@tanstack/react-router";
import { ChevronRight, PanelLeft } from "lucide-react";
import * as React from "react";

import { IconButton, Tooltip } from "@pidash/kit";

import { buildAdminCrumbs } from "../adminShell.js";
import { useAdminSidebarStore } from "../stores.js";

type RouterLinkProps = Parameters<typeof Link>[0];

// Console pages land slice by slice, so most crumb targets are not in
// the route tree yet; the cast carries the literal through the same
// way the first-workspace shortcut reaches its out-of-tree page.
function crumbLinkProps(href: string): RouterLinkProps {
  return { to: href } as RouterLinkProps;
}

export function AdminHeader(): React.ReactElement {
  const toggleCollapsed = useAdminSidebarStore((state) => state.toggleCollapsed);
  const { pathname } = useLocation();
  const crumbs = buildAdminCrumbs(pathname);
  return (
    <header
      data-testid="admin-header"
      className="flex w-full flex-shrink-0 items-center gap-(--space-2) border-b border-(--border) bg-(--surface) p-(--space-4)"
    >
      <IconButton
        label="Open sidebar"
        size="small"
        data-testid="admin-hamburger"
        onClick={toggleCollapsed}
        className="md:hidden"
      >
        <PanelLeft size={16} aria-hidden="true" />
      </IconButton>
      <nav aria-label="Breadcrumb" data-testid="admin-breadcrumbs" className="min-w-0 flex-1">
        <ol className="flex min-w-0 items-center gap-(--space-1)">
          {crumbs.map((crumb, index) => (
            <li key={`${index}-${crumb.label}`} className="flex min-w-0 items-center gap-(--space-1)">
              {index > 0 ? (
                <ChevronRight size={14} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
              ) : null}
              <Tooltip content={crumb.label}>
                <span className="flex min-w-0 items-center">
                  <Link
                    {...crumbLinkProps(crumb.href)}
                    className="text-body block max-w-40 truncate text-(--text-muted) hover:text-(--text)"
                  >
                    {crumb.label}
                  </Link>
                </span>
              </Tooltip>
            </li>
          ))}
        </ol>
      </nav>
    </header>
  );
}
