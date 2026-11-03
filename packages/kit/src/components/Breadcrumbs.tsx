// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Breadcrumb trail (SHELL-045). One crumb per trail entry: entries with a
// target render as links. The terminal entry renders as plain text with
// aria-current, unless `terminalAsLink` asks for a self-referencing
// address (a screen whose own address leads back from a nested
// selection). Long labels truncate with their full text on hover; entries
// may carry a leading icon.
import { ChevronRight } from "lucide-react";
import * as React from "react";
import { cn } from "../lib/cn";

export interface BreadcrumbEntry {
  /** Visible label. */
  label: string;
  /** Link target. Absent entries render as plain text. */
  href?: string;
  /** Leading glyph, rendered decoratively. */
  icon?: React.ReactNode;
}

/** Labels longer than this keep their full text in a hover title. */
export const BREADCRUMB_LONG_LABEL_LENGTH = 24;

export interface BreadcrumbsProps {
  entries: BreadcrumbEntry[];
  /** Accessible name of the trail landmark. */
  label?: string;
  className?: string;
  /**
   * Render the terminal entry as a link when it carries a target, keeping
   * aria-current on it. Defaults to plain text for the current location.
   */
  terminalAsLink?: boolean;
}

export function Breadcrumbs({
  entries,
  label = "Breadcrumb",
  className,
  terminalAsLink = false,
}: BreadcrumbsProps): React.ReactElement | null {
  if (entries.length === 0) return null;
  const terminal = entries.length - 1;
  return (
    <nav aria-label={label} className={className}>
      <ol className="flex min-w-0 items-center gap-(--space-1)">
        {entries.map((entry, index) => {
          const last = index === terminal;
          const linked = entry.href !== undefined && (!last || terminalAsLink);
          const long = entry.label.length > BREADCRUMB_LONG_LABEL_LENGTH;
          return (
            <li key={`${index}-${entry.label}`} className="flex min-w-0 items-center gap-(--space-1)">
              {index > 0 ? (
                <ChevronRight size={14} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
              ) : null}
              {linked ? (
                <a
                  href={entry.href}
                  aria-current={last ? "page" : undefined}
                  className={cn(
                    "text-body flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control)",
                    last ? "text-(--text)" : "text-(--text-muted) hover:text-(--text)"
                  )}
                >
                  <CrumbBody entry={entry} long={long} />
                </a>
              ) : (
                <span
                  aria-current={last ? "page" : undefined}
                  className={cn(
                    "text-body flex min-w-0 items-center gap-(--space-1)",
                    last ? "text-(--text)" : "text-(--text-muted)"
                  )}
                >
                  <CrumbBody entry={entry} long={long} />
                </span>
              )}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

function CrumbBody({ entry, long }: { entry: BreadcrumbEntry; long: boolean }): React.ReactElement {
  return (
    <>
      {entry.icon === undefined ? null : (
        <span aria-hidden="true" className="flex shrink-0 items-center">
          {entry.icon}
        </span>
      )}
      <span {...(long ? { title: entry.label } : {})} className="max-w-40 truncate">
        {entry.label}
      </span>
    </>
  );
}
