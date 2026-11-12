// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module links section (parity/specs/modules.md §9.3, MOD-035). The
// bottom section of the analytics sidebar: a disclosure that stays
// collapsed while empty and opens by default once links exist — the
// default is computed from the loaded links on every mount, so a reload
// with links opens it (NEWFRONT-342). Rows show a type icon, the title
// or URL opening in a new tab, a copy action and the age with its
// author; add, edit and delete belong to editors only, and guests and
// read-only mounts keep the rows with open and copy.

import { useQuery } from "@tanstack/react-query";
import type { ApiClient, WorkspaceMemberList } from "@pidash/api-client";
import { ChevronDown, Copy, ExternalLink, Globe, Pencil, Plus, Trash2 } from "lucide-react";
import * as React from "react";

import { IconButton, Tooltip } from "@pidash/kit";

import { getAppClient } from "../../../../core/api/client.js";
import { workspaceMembersQueryOptions } from "../../../projects/index.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { ModuleLinkDialog, type ModuleLinkDraft } from "./ModuleLinkDialog.js";
import {
  MODULE_LINK_ADD_TITLE,
  MODULE_LINK_COPIED,
  MODULE_LINK_COPY_FAILED_TITLE,
  MODULE_LINKS_EMPTY,
  MODULE_LINKS_TITLE,
  describeModuleLinkMeta,
  moduleLinkFavicon,
  moduleLinkTitle,
  moduleLinksNeedAuthors,
  resolveModuleLinkAuthor,
  type ModuleLinkSummary,
} from "./moduleLinksModel.js";
import { useModuleLinksWrite } from "./useModuleLinksWrite.js";

export interface ModuleLinksSectionProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  /** The detail record's links, in the order it carries them. */
  links: readonly ModuleLinkSummary[];
  /** False for guests, read-only mounts and archived modules. */
  canEdit: boolean;
  /** Test seam; the app client by default. */
  client?: ApiClient;
}

export function ModuleLinksSection({
  workspaceSlug,
  projectId,
  moduleId,
  links,
  canEdit,
  client = getAppClient(),
}: ModuleLinksSectionProps): React.ReactElement {
  const [userOpen, setUserOpen] = React.useState<boolean | null>(null);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<ModuleLinkSummary | null>(null);
  const open = userOpen ?? links.length > 0;
  const writes = useModuleLinksWrite({ client, workspaceSlug, projectId, moduleId });
  const members = useQuery({
    ...workspaceMembersQueryOptions(client, workspaceSlug),
    enabled: moduleLinksNeedAuthors(links),
  });

  function openAdd(): void {
    setEditing(null);
    setDialogOpen(true);
  }

  function openEdit(link: ModuleLinkSummary): void {
    setEditing(link);
    setDialogOpen(true);
  }

  function submit(draft: ModuleLinkDraft): void {
    const written = (): void => {
      setDialogOpen(false);
      setEditing(null);
    };
    if (editing === null) {
      writes.createLink.mutate({ url: draft.url, title: draft.title }, { onSuccess: written });
    } else {
      writes.updateLink.mutate(
        { linkId: editing.id, input: { url: draft.url, title: draft.title } },
        { onSuccess: written }
      );
    }
  }

  async function copy(url: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(url);
      notifySuccess(MODULE_LINK_COPIED);
    } catch (error) {
      notifyError(MODULE_LINK_COPY_FAILED_TITLE, (error as Error).message);
    }
  }

  return (
    <section
      aria-label={MODULE_LINKS_TITLE}
      data-testid="modules-links-section"
      className="flex flex-col gap-(--space-2) border-t border-(--border) pt-(--space-4)"
    >
      <div className="flex items-center justify-between gap-(--space-2)">
        <button
          type="button"
          data-testid="modules-links-toggle"
          aria-expanded={open}
          onClick={() => setUserOpen(!open)}
          className="flex min-w-0 flex-1 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle)"
        >
          <ChevronDown
            size={14}
            aria-hidden="true"
            className={open ? "shrink-0 rotate-180 text-(--text-muted)" : "shrink-0 text-(--text-muted)"}
          />
          <span className="text-body font-medium text-(--text)">{MODULE_LINKS_TITLE}</span>
        </button>
        {canEdit ? (
          <button
            type="button"
            data-testid="modules-links-add"
            onClick={openAdd}
            className="text-body inline-flex shrink-0 items-center gap-(--space-1) rounded-(--radius-control) px-(--space-2) py-(--space-1) font-medium text-(--accent) hover:bg-(--subtle)"
          >
            <Plus size={14} aria-hidden="true" />
            {MODULE_LINK_ADD_TITLE}
          </button>
        ) : null}
      </div>
      {open ? (
        links.length === 0 ? (
          <p data-testid="modules-links-empty" className="text-body px-(--space-2) text-(--text-muted)">
            {MODULE_LINKS_EMPTY}
          </p>
        ) : (
          <ul className="flex flex-col gap-(--space-2)">
            {links.map((link) => (
              <ModuleLinkRow
                key={link.id}
                link={link}
                members={members.data}
                canEdit={canEdit}
                onEdit={openEdit}
                onDelete={(linkId) => writes.deleteLink.mutate(linkId)}
                onCopy={(url) => void copy(url)}
              />
            ))}
          </ul>
        )
      ) : null}
      {canEdit ? (
        <ModuleLinkDialog
          open={dialogOpen}
          onOpenChange={setDialogOpen}
          link={editing}
          pending={editing === null ? writes.createLink.isPending : writes.updateLink.isPending}
          onSubmit={submit}
        />
      ) : null}
    </section>
  );
}

interface ModuleLinkRowProps {
  link: ModuleLinkSummary;
  members: WorkspaceMemberList | undefined;
  canEdit: boolean;
  onEdit: (link: ModuleLinkSummary) => void;
  onDelete: (linkId: string) => void;
  onCopy: (url: string) => void;
}

function ModuleLinkRow({ link, members, canEdit, onEdit, onDelete, onCopy }: ModuleLinkRowProps): React.ReactElement {
  const title = moduleLinkTitle(link);
  const icon = moduleLinkFavicon(link.url);
  const meta = describeModuleLinkMeta(link.created_at, resolveModuleLinkAuthor(members, link.created_by));
  return (
    <li
      data-testid="modules-link-row"
      className="flex flex-col gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-3) py-(--space-2)"
    >
      <div className="flex items-center gap-(--space-2)">
        <Tooltip content={title}>
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
            <span className="text-body min-w-0 flex-1 truncate text-(--text)">{title}</span>
            <ExternalLink size={12} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          </a>
        </Tooltip>
        <div className="flex shrink-0 items-center">
          <IconButton
            label="Copy link"
            size="small"
            variant="ghost"
            data-testid="modules-link-copy"
            onClick={() => onCopy(link.url)}
          >
            <Copy size={14} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
          {canEdit ? (
            <IconButton
              label="Edit link"
              size="small"
              variant="ghost"
              data-testid="modules-link-edit"
              onClick={() => onEdit(link)}
            >
              <Pencil size={14} strokeWidth={1.5} aria-hidden="true" />
            </IconButton>
          ) : null}
          {canEdit ? (
            <IconButton
              label="Delete link"
              size="small"
              variant="ghost"
              data-testid="modules-link-delete"
              onClick={() => onDelete(link.id)}
            >
              <Trash2 size={14} strokeWidth={1.5} aria-hidden="true" />
            </IconButton>
          ) : null}
        </div>
      </div>
      {meta === null ? null : <p className="text-meta pl-(--space-6) text-(--text-muted)">{meta}</p>}
    </li>
  );
}
