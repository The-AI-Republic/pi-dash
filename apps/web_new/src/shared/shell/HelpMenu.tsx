// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Help menu (SHELL-060, from parity/specs/shell.md §4). Presentational:
// the gated route passes the app version and every opener. External
// entries leave the app in a new tab through the route's opener; the
// shortcut list and the product-updates dialog open in place.
import { IconButton, Menu } from "@pidash/kit";
import { BookOpen, CircleHelp, Keyboard, Mail, MessagesSquare, Newspaper } from "lucide-react";
import * as React from "react";

import { DOCS_URL, FORUM_URL } from "./externalLinks.js";

/** The sales contact behind the menu's contact entry. */
export const SALES_EMAIL_URL = "mailto:sales@example.com";

export interface HelpMenuProps {
  version: string;
  onOpenShortcuts: () => void;
  onOpenUpdates: () => void;
  /** Opens a URL outside the app (system browser on desktop). */
  onOpenExternal: (url: string) => void;
}

export function HelpMenu({
  version,
  onOpenShortcuts,
  onOpenUpdates,
  onOpenExternal,
}: HelpMenuProps): React.ReactElement {
  return (
    <Menu
      trigger={
        <IconButton label="Help" data-testid="topbar-help-menu">
          <CircleHelp size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      }
      items={[
        {
          label: "Documentation",
          onSelect: () => onOpenExternal(DOCS_URL),
          icon: <BookOpen size={16} strokeWidth={1.5} aria-hidden="true" />,
        },
        {
          label: "Contact sales",
          onSelect: () => onOpenExternal(SALES_EMAIL_URL),
          icon: <Mail size={16} strokeWidth={1.5} aria-hidden="true" />,
        },
        { separator: true },
        {
          label: "Keyboard shortcuts",
          onSelect: onOpenShortcuts,
          icon: <Keyboard size={16} strokeWidth={1.5} aria-hidden="true" />,
        },
        {
          label: "What's new?",
          onSelect: onOpenUpdates,
          icon: <Newspaper size={16} strokeWidth={1.5} aria-hidden="true" />,
        },
        {
          label: "Forum",
          onSelect: () => onOpenExternal(FORUM_URL),
          icon: <MessagesSquare size={16} strokeWidth={1.5} aria-hidden="true" />,
        },
        {
          custom: true,
          entryKey: "version",
          node: (
            <p
              data-testid="help-menu-version"
              className="text-caption px-(--space-4) pt-(--space-2) text-(--text-muted)"
            >
              Version: v{version}
            </p>
          ),
        },
      ]}
    />
  );
}
