// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Links widget (parity/specs/issues.md §15, ISS-184/186). Each card shows
// the favicon derived from the URL, the stored title or the URL itself,
// and its attribution. The row anchor opens the address in a new tab;
// the copy control writes the URL to the clipboard; the menu edits or
// removes without a confirmation step.

import { Copy, Ellipsis, ExternalLink, Globe, Pencil, Trash2 } from "lucide-react";
import * as React from "react";

import { IconButton, Menu } from "@pidash/kit";
import type { IssueLink } from "@pidash/api-client";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { LinkDialog, type LinkDraft } from "./LinkDialog.js";
import { useCreateLink, useDeleteLink, useIssueLinks, useUpdateLink, newestLinks } from "./api.js";
import { WidgetAddButton, WidgetSection } from "./WidgetSection.js";

/** Host portion of an absolute URL; empty for anything unparseable. */
export function faviconUrl(url: string): string | null {
  try {
    const parsed = new URL(url);
    return `${parsed.origin}/favicon.ico`;
  } catch {
    return null;
  }
}

/** One row's display text: the title when set, else the URL. */
export function linkTitle(link: IssueLink): string {
  const title = link.title?.trim() ?? "";
  return title === "" ? link.url : title;
}

const formatter = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
const MINUTE = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;

/** Short relative age; "just now" inside the first minute. */
export function addedAgo(createdAt: string, nowMs: number = Date.now()): string {
  const then = Date.parse(createdAt);
  if (Number.isNaN(then)) return "just now";
  const elapsed = nowMs - then;
  if (elapsed < MINUTE) return "just now";
  if (elapsed < HOUR) return formatter.format(-Math.floor(elapsed / MINUTE), "minute");
  if (elapsed < DAY) return formatter.format(-Math.floor(elapsed / HOUR), "hour");
  if (elapsed < 7 * DAY) return formatter.format(-Math.floor(elapsed / DAY), "day");
  return formatter.format(-Math.floor(elapsed / (30 * DAY)), "month");
}

/** "Added 5 minutes ago by Ada"; bots carry their own label. */
export function attribution(link: IssueLink, nowMs: number = Date.now()): string {
  const author = link.created_by_detail;
  const who = author === null ? null : author.is_bot ? `${author.display_name} (bot)` : author.display_name;
  return who === null
    ? `Added ${addedAgo(link.created_at, nowMs)}`
    : `Added ${addedAgo(link.created_at, nowMs)} by ${who}`;
}

export interface LinksWidgetProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  canEdit: boolean;
}

export function LinksWidget({
  workspaceSlug,
  projectId,
  issueId,
  canEdit,
}: LinksWidgetProps): React.ReactElement | null {
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<IssueLink | null>(null);
  const links = useIssueLinks(workspaceSlug, projectId, issueId);
  const create = useCreateLink({ workspaceSlug, projectId, issueId });
  const update = useUpdateLink({ workspaceSlug, projectId, issueId });
  const remove = useDeleteLink({ workspaceSlug, projectId, issueId });
  const rows = newestLinks(links.data);

  if (rows.length === 0 && !canEdit) return null;

  function openAdd(): void {
    setEditing(null);
    setDialogOpen(true);
  }

  function openEdit(link: IssueLink): void {
    setEditing(link);
    setDialogOpen(true);
  }

  function submit(draft: LinkDraft): void {
    const done = (): void => {
      setDialogOpen(false);
    };
    if (editing === null) {
      create.mutate({ url: draft.url, title: draft.title === "" ? null : draft.title }, { onSuccess: done });
    } else {
      update.mutate(
        { linkId: editing.id, patch: { url: draft.url, title: draft.title === "" ? null : draft.title } },
        { onSuccess: done }
      );
    }
  }

  async function copy(url: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(url);
      notifySuccess("Link copied");
    } catch (error) {
      notifyError("Couldn't copy the link", (error as Error).message);
    }
  }

  const pending = create.isPending || update.isPending;

  return (
    <>
      <WidgetSection
        title="Links"
        count={rows.length}
        add={canEdit ? <WidgetAddButton label="Add link" onClick={openAdd} /> : undefined}
      >
        <div className="flex flex-col gap-(--space-1) p-(--space-1)">
          {rows.length === 0 ? (
            <p className="text-body px-(--space-2) py-(--space-3) text-(--text-muted)">No links yet.</p>
          ) : (
            rows.map((link) => {
              const icon = faviconUrl(link.url);
              return (
                <div
                  key={link.id}
                  data-testid={`link-row-${link.id}`}
                  className="flex min-h-10 items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-3) py-(--space-1)"
                >
                  <a
                    href={link.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="flex min-w-0 flex-1 items-center gap-(--space-2)"
                  >
                    {icon === null ? (
                      <Globe size={14} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
                    ) : (
                      <img src={icon} alt="" aria-hidden="true" className="size-4 shrink-0" />
                    )}
                    <span className="text-body min-w-0 flex-1 truncate text-(--text)">{linkTitle(link)}</span>
                    <ExternalLink
                      size={12}
                      strokeWidth={1.5}
                      aria-hidden="true"
                      className="shrink-0 text-(--text-muted)"
                    />
                  </a>
                  <span className="text-meta shrink-0 text-(--text-muted)">{attribution(link)}</span>
                  <IconButton
                    label={`Copy link ${linkTitle(link)}`}
                    size="small"
                    variant="ghost"
                    onClick={() => void copy(link.url)}
                  >
                    <Copy size={14} strokeWidth={1.5} aria-hidden="true" />
                  </IconButton>
                  {canEdit ? (
                    <Menu
                      trigger={
                        <IconButton label={`Link actions for ${linkTitle(link)}`} size="small" variant="ghost">
                          <Ellipsis size={14} strokeWidth={1.5} aria-hidden="true" />
                        </IconButton>
                      }
                      items={[
                        {
                          label: "Edit",
                          icon: <Pencil size={14} aria-hidden="true" />,
                          onSelect: () => openEdit(link),
                        },
                        {
                          label: "Delete",
                          icon: <Trash2 size={14} aria-hidden="true" />,
                          onSelect: () => remove.mutate(link.id),
                        },
                      ]}
                    />
                  ) : null}
                </div>
              );
            })
          )}
        </div>
      </WidgetSection>
      {canEdit ? (
        <LinkDialog open={dialogOpen} onOpenChange={setDialogOpen} link={editing} pending={pending} onSubmit={submit} />
      ) : null}
    </>
  );
}
