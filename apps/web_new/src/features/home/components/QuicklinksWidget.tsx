// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved reference-link widget (SHELL-011, 013; written from
// parity/specs/shell.md §2). Each row opens its address in a new tab and
// offers copy, edit and delete through its menu; every write confirms
// with a notice. Loading shows skeleton tiles, zero links show guidance,
// and long lists collapse behind an expander. A failed list fetch keeps
// prior rows with no retry affordance.

import { Ellipsis, Link2, Plus } from "lucide-react";
import * as React from "react";

import type { QuickLink } from "@pidash/api-client";
import { Button, IconButton, Menu, Skeleton } from "@pidash/kit";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useDeleteQuickLink, useQuickLinks } from "../api/quicklinks.js";
import { quickLinkDisplayName } from "../lib/quicklinks.js";
import { formatRelativeVisitTime } from "../lib/relativeTime.js";
import { QuickLinkDialog } from "./QuickLinkDialog.js";

/** Rows before the list caps behind the expander. */
const COLLAPSED_QUICKLINKS = 5;

function QuicklinksEmptyState(): React.ReactElement {
  return (
    <div
      data-testid="quicklinks-empty"
      className="flex flex-col items-center gap-(--space-2) px-(--space-4) py-(--space-8) text-center"
    >
      <span
        aria-hidden="true"
        className="grid size-10 place-items-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted)"
      >
        <Link2 size={20} strokeWidth={1.5} aria-hidden="true" />
      </span>
      <p className="text-emphasis text-(--text)">No saved links yet</p>
      <p className="text-body text-(--text-muted)">Save addresses you open often for one-click access.</p>
    </div>
  );
}

export function QuicklinksWidget({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement {
  const links = useQuickLinks(workspaceSlug);
  const remove = useDeleteQuickLink(workspaceSlug);
  const [expanded, setExpanded] = React.useState(false);
  const [dialog, setDialog] = React.useState<{ link: QuickLink | null } | null>(null);
  const rows = links.data ?? [];
  const visible = expanded ? rows : rows.slice(0, COLLAPSED_QUICKLINKS);

  async function copyAddress(link: QuickLink): Promise<void> {
    try {
      const clipboard = globalThis.navigator.clipboard;
      if (clipboard === undefined) throw new Error("clipboard unavailable");
      await clipboard.writeText(link.url);
      notifySuccess("Link address copied");
    } catch {
      notifyError("Link copy failed");
    }
  }

  return (
    <section
      id="home-quicklinks"
      aria-label="Quicklinks"
      data-widget-section="quicklinks"
      data-widget-title="Quicklinks"
      className="flex flex-col gap-(--space-3)"
    >
      <div className="flex items-center justify-between gap-(--space-3)">
        <h2 className="text-emphasis text-(--text)">Quicklinks</h2>
        <IconButton
          label="Add link"
          size="small"
          onClick={() => {
            setDialog({ link: null });
          }}
        >
          <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
      {links.isPending ? (
        <div data-testid="quicklinks-skeleton" aria-busy="true" className="flex flex-col gap-(--space-2)">
          <Skeleton height="44px" />
          <Skeleton height="44px" />
          <Skeleton height="44px" />
        </div>
      ) : rows.length > 0 ? (
        <>
          <ul aria-label="Saved links" className="flex flex-col">
            {visible.map((link) => {
              const name = quickLinkDisplayName(link);
              return (
                <li
                  key={link.id}
                  className="flex items-center justify-between gap-(--space-3) border-b border-(--border) py-(--space-3) last:border-b-0"
                >
                  <a
                    href={link.url}
                    target="_blank"
                    rel="noreferrer"
                    className="flex min-w-0 flex-col gap-(--space-1) rounded-(--radius-control)"
                  >
                    <span data-testid="quicklink-title" className="text-body truncate text-(--text)">
                      {name}
                    </span>
                    <span className="text-meta text-(--text-muted)">{formatRelativeVisitTime(link.updated_at)}</span>
                  </a>
                  <Menu
                    trigger={
                      <IconButton label={`Link actions for ${name}`} size="small">
                        <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
                      </IconButton>
                    }
                    items={[
                      {
                        label: "Copy address",
                        onSelect: () => {
                          void copyAddress(link);
                        },
                      },
                      {
                        label: "Edit",
                        onSelect: () => {
                          setDialog({ link });
                        },
                      },
                      {
                        label: "Delete",
                        onSelect: () => {
                          remove.mutate(link.id);
                        },
                      },
                    ]}
                  />
                </li>
              );
            })}
          </ul>
          {rows.length > COLLAPSED_QUICKLINKS ? (
            <Button variant="secondary" size="small" onClick={() => setExpanded((value) => !value)}>
              {expanded ? "Show less" : "Show all"}
            </Button>
          ) : null}
        </>
      ) : links.isError ? null : (
        <QuicklinksEmptyState />
      )}
      {dialog === null ? null : (
        <QuickLinkDialog
          key={dialog.link?.id ?? "new"}
          workspaceSlug={workspaceSlug}
          link={dialog.link}
          open
          onOpenChange={() => {
            setDialog(null);
          }}
        />
      )}
    </section>
  );
}
