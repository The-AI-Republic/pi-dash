// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quick-action behavior (parity/specs/issues.md §7, ISS-060–067). The
// hook owns every menu's side effects: opening the edit and copy forms,
// the archive/delete/move confirmations built by NEWFRONT-632, the link
// copy, the deep-link open, the cycle/module bridge deletes and the
// restore. It returns the menu entries the component renders, plus the
// dialogs that must mount beside the trigger.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { restoreIssue, removeIssueFromCycle, setIssueModules, type IssueListItem } from "@pidash/api-client";
import { Archive, ArrowUpRight, Copy, FolderInput, Link2, Pencil, RotateCcw, Trash2, XCircle } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useMe } from "../../../core/session/index.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { copyLinkWithToast } from "../../../shared/shell/shareLink.js";
import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import { usePickerStates } from "../../../shared/pickers/reference.js";
import { issueKeys } from "../api/keys.js";
import { WorkItemCreateDialog, type WorkItemCreateProject } from "../components/WorkItemCreateDialog.js";
import { ArchiveWorkItemDialog } from "../lifecycle/ArchiveWorkItemDialog.js";
import { DeleteWorkItemDialog } from "../lifecycle/DeleteWorkItemDialog.js";
import { MoveWorkItemDialog } from "../lifecycle/MoveWorkItemDialog.js";
import {
  COPY_LINK_COPIED_TITLE,
  COPY_LINK_COPY_FAILED_TITLE,
  RESTORE_FAILED_BODY,
  RESTORE_FAILED_TITLE,
  RESTORE_SUCCESS_BODY,
  RESTORE_SUCCESS_TITLE,
  REMOVE_FAILED_BODY,
  REMOVE_FAILED_TITLE,
  removeSuccessTitle,
  workItemBrowseHref,
} from "./messages.js";
import { canArchiveStateGroup, quickActionEntries, type QuickActionContext, type QuickActionKey } from "./model.js";

/** The one record a menu acts on. */
export interface QuickActionTarget {
  id: string;
  name: string;
  sequenceId: number;
  projectId: string;
  /** Short project code the menus show their key with. */
  projectIdentifier: string;
  /** The record's workflow state, when the caller already holds it. */
  stateId: string | null;
  /** State group when the caller already knows it (the global list does). */
  stateGroup?: string | null;
  /** The row as the caller holds it, so edit and copy can prefill the form. */
  snapshot?: unknown;
  createdBy?: string | null;
  archived?: boolean;
  isEpic?: boolean;
}

export interface QuickActionsInput {
  workspaceSlug: string;
  target: QuickActionTarget;
  /** Which surface opened the menu. */
  context: QuickActionContext;
  /** Cycle or module the row belongs to, for the remove entry and prefill. */
  cycleId?: string | null;
  moduleId?: string | null;
  /** The viewer may write on this project (archived projects and guests read). */
  canWrite?: boolean;
  /** Caller's refresh after a landed write; the hook also invalidates reads. */
  onChanged?: (() => void) | undefined;
  /** After a landed restore (the archived list usually refetches). */
  onRestored?: (() => void) | undefined;
  /** After a landed archive (a detail page navigates back to its list). */
  onArchived?: (() => void) | undefined;
  /** After a landed delete (a detail page navigates back to its list). */
  onDeleted?: (() => void) | undefined;
}

export interface QuickActionHandlers {
  run: (key: QuickActionKey) => void;
}

export interface QuickActionsState {
  entries: ReturnType<typeof quickActionEntries>;
  handlers: QuickActionHandlers;
  /** Dialogs and forms the trigger must mount; render beside the menu. */
  overlays: React.ReactElement | null;
}

/** Row fields the create form reads when it opens on a copy. */
export function copySeed(target: QuickActionTarget): Record<string, unknown> {
  const row = typeof target.snapshot === "object" && target.snapshot !== null ? target.snapshot : {};
  return { ...row, id: target.id, name: `${target.name} (copy)` };
}

type ProjectRow = {
  id: string;
  name: string;
  identifier: string;
  member_role: number | null;
  archived_at: unknown;
};

/** Project choices the move dialog lists: joined, live, with a code. */
export function moveTargetsFrom(projects: readonly ProjectRow[]): { id: string; name: string; identifier: string }[] {
  return projects
    .filter((project) => project.member_role !== null && project.archived_at == null && project.identifier !== "")
    .map((project) => ({ id: project.id, name: project.name, identifier: project.identifier }));
}

const ICONS: Record<QuickActionKey, React.ReactNode> = {
  edit: <Pencil size={14} aria-hidden="true" />,
  copy: <Copy size={14} aria-hidden="true" />,
  openInNewTab: <ArrowUpRight size={14} aria-hidden="true" />,
  copyLink: <Link2 size={14} aria-hidden="true" />,
  move: <FolderInput size={14} aria-hidden="true" />,
  archive: <Archive size={14} aria-hidden="true" />,
  restore: <RotateCcw size={14} aria-hidden="true" />,
  delete: <Trash2 size={14} aria-hidden="true" />,
  removeFromCycle: <XCircle size={14} aria-hidden="true" />,
  removeFromModule: <XCircle size={14} aria-hidden="true" />,
};

/** The leading mark a menu entry shows. */
export function quickActionIcon(key: QuickActionKey): React.ReactNode {
  return ICONS[key];
}

type DialogOpen = "edit" | "copy" | "archive" | "delete" | "move" | null;

/** Placeholder while the caller has no target: every query stays off. */
const IDLE_TARGET: QuickActionTarget = {
  id: "",
  name: "",
  sequenceId: 0,
  projectId: "",
  projectIdentifier: "",
  stateId: null,
};

/**
 * Bind the menu model to its writes. The project list (move targets and
 * the viewer's standing) loads only while the menu is mounted; the state
 * group behind the archive gate is read from the picker cache the list
 * strip already fills.
 */
export function useQuickActions(input: QuickActionsInput): QuickActionsState;
export function useQuickActions(input: null): null;
export function useQuickActions(input: QuickActionsInput | null): QuickActionsState | null;
export function useQuickActions(input: QuickActionsInput | null): QuickActionsState | null {
  const client = getAppClient();
  const queryClient = useQueryClient();
  const me = useMe();
  const workspaceSlug = input?.workspaceSlug ?? "";
  const projects = useQuery({ ...projectsQueryOptions(client, workspaceSlug), enabled: input !== null });
  const [open, setOpen] = React.useState<DialogOpen>(null);
  const target = input?.target ?? IDLE_TARGET;
  const project = projects.data?.find((row) => row.id === target.projectId) ?? null;
  const canWrite =
    input?.canWrite ??
    (project !== null
      ? canEditInProject({ memberRole: project.member_role, archived: project.archived_at != null })
      : false);
  const stateGroup = useMenuStateGroup(workspaceSlug, target, input !== null);
  const entries = quickActionEntries({
    context: input?.context ?? "project",
    canWrite,
    archivable: canArchiveStateGroup(stateGroup),
    archived: target.archived === true,
  });

  const refresh = React.useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    input?.onChanged?.();
  }, [queryClient, input, workspaceSlug]);

  const href = workItemBrowseHref(workspaceSlug, target.projectIdentifier, target.sequenceId);

  const copyLink = React.useCallback(() => {
    void copyLinkWithToast(
      href,
      { copied: COPY_LINK_COPIED_TITLE, failed: COPY_LINK_COPY_FAILED_TITLE },
      { success: notifySuccess, error: notifyError }
    );
  }, [href]);

  const openInNewTab = React.useCallback(() => {
    globalThis.window.open(href, "_blank", "noopener,noreferrer");
  }, [href]);

  const removeMembership = React.useCallback(
    (kind: "cycle" | "module") => {
      const memberId = kind === "cycle" ? input?.cycleId : input?.moduleId;
      if (memberId == null || memberId === "") {
        notifyError(REMOVE_FAILED_TITLE, REMOVE_FAILED_BODY);
        return;
      }
      const request =
        kind === "cycle"
          ? removeIssueFromCycle(client, workspaceSlug, target.projectId, memberId, target.id)
          : setIssueModules(client, workspaceSlug, target.projectId, target.id, {
              modules: [],
              removed_modules: [memberId],
            }).then(() => undefined);
      void request.then(
        () => {
          notifySuccess(removeSuccessTitle(kind));
          refresh();
        },
        () => notifyError(REMOVE_FAILED_TITLE, REMOVE_FAILED_BODY)
      );
    },
    [client, input, workspaceSlug, refresh, target.id, target.projectId]
  );

  const restore = React.useCallback(() => {
    void restoreIssue(client, workspaceSlug, target.projectId, target.id).then(
      () => {
        notifySuccess(RESTORE_SUCCESS_TITLE, RESTORE_SUCCESS_BODY);
        refresh();
        input?.onRestored?.();
      },
      () => notifyError(RESTORE_FAILED_TITLE, RESTORE_FAILED_BODY)
    );
  }, [client, input, refresh, target.id, target.projectId, workspaceSlug]);

  const run = React.useCallback(
    (key: QuickActionKey) => {
      switch (key) {
        case "edit":
        case "copy":
        case "archive":
        case "delete":
        case "move":
          setOpen(key);
          return;
        case "openInNewTab":
          openInNewTab();
          return;
        case "copyLink":
          copyLink();
          return;
        case "restore":
          restore();
          return;
        case "removeFromCycle":
          removeMembership("cycle");
          return;
        case "removeFromModule":
          removeMembership("module");
          return;
      }
    },
    [copyLink, openInNewTab, removeMembership, restore]
  );

  const close = React.useCallback((): void => setOpen(null), []);

  /** Every dialog closes on a false `onOpenChange`; a true one keeps it up. */
  const dismiss = React.useCallback(
    (next: boolean): void => {
      if (!next) close();
    },
    [close]
  );

  /** A landed write closes whichever dialog ran it and re-reads the lists. */
  const afterWrite = React.useCallback((): void => {
    close();
    refresh();
  }, [close, refresh]);

  // Detail follow-ups after the shared close-and-refresh: the mounting
  // page navigates away from a record it just archived or deleted.
  const onArchived = input?.onArchived;
  const onDeleted = input?.onDeleted;
  const afterArchive = React.useCallback((): void => {
    afterWrite();
    onArchived?.();
  }, [afterWrite, onArchived]);
  const afterDelete = React.useCallback((): void => {
    afterWrite();
    onDeleted?.();
  }, [afterWrite, onDeleted]);

  if (input === null) return null;

  const createContext = quickActionCreateContext(input.cycleId, input.moduleId, open);
  // A duplicate opens the create form on the source row under its
  // "(copy)" title; nothing reaches the server until Save (ISS-060).
  const copyPrefill = copySeed(target);
  const overlay = (
    <>
      {open === "edit" || open === "copy" ? (
        <WorkItemCreateDialog
          open
          onOpenChange={dismiss}
          workspaceSlug={input.workspaceSlug}
          projects={createProjects(projects.data)}
          initialProjectId={target.projectId}
          projectIdentifier={target.projectIdentifier}
          canCreate={canWrite}
          {...(open === "copy" ? { copySeed: { title: String(copyPrefill["name"]), snapshot: copyPrefill } } : {})}
          {...(open === "edit"
            ? {
                edit: {
                  kind: "issue" as const,
                  id: target.id,
                  projectId: target.projectId,
                  snapshot: target.snapshot ?? { id: target.id, name: target.name },
                  onChanged: refresh,
                },
              }
            : {})}
          {...(createContext === undefined ? {} : { context: createContext })}
          onCreated={afterWrite}
        />
      ) : null}
      {open === "archive" ? (
        <ArchiveWorkItemDialog
          open
          onOpenChange={dismiss}
          scope={{ workspaceSlug: input.workspaceSlug, projectId: target.projectId, issueId: target.id }}
          projectIdentifier={target.projectIdentifier}
          sequenceId={target.sequenceId}
          kind={target.isEpic === true ? "epic" : "issue"}
          onArchived={afterArchive}
        />
      ) : null}
      {open === "delete" ? (
        <DeleteWorkItemDialog
          open
          onOpenChange={dismiss}
          scope={{ workspaceSlug: input.workspaceSlug, projectId: target.projectId, issueId: target.id }}
          projectIdentifier={target.projectIdentifier}
          sequenceId={target.sequenceId}
          kind={target.isEpic === true ? "epic" : "issue"}
          viewerId={me.data?.id ?? null}
          creatorId={target.createdBy ?? null}
          projectRole={project?.member_role ?? null}
          onDeleted={afterDelete}
        />
      ) : null}
      {open === "move" ? (
        <MoveWorkItemDialog
          open
          onOpenChange={dismiss}
          scope={{ workspaceSlug: input.workspaceSlug, projectId: target.projectId, issueId: target.id }}
          projects={moveTargetsFrom(projects.data ?? [])}
          identifierOf={(projectId) => projects.data?.find((row) => row.id === projectId)?.identifier}
        />
      ) : null}
    </>
  );

  return { entries, handlers: { run }, overlays: overlay };
}

/**
 * The workflow state group behind a row. A caller that already holds the
 * group (the global list's annotation) wins; otherwise the states the
 * list strip reads answer by the row's state id. Unknown is not
 * archivable, so the entry stays disabled until the answer arrives.
 */
export function useMenuStateGroup(
  workspaceSlug: string,
  target: Pick<QuickActionTarget, "projectId" | "stateId" | "stateGroup" | "archived">,
  enabled = true
): string | null {
  const known = target.stateGroup;
  const states = usePickerStates(workspaceSlug, target.projectId, enabled && known == null && target.stateId !== null);
  if (known != null) return known;
  if (target.stateId === null) return null;
  return states.data?.find((row) => row.id === target.stateId)?.group ?? null;
}

function createProjects(rows: readonly ProjectRow[] | undefined): WorkItemCreateProject[] {
  return (rows ?? []).map((row) => ({
    id: row.id,
    name: row.name,
    identifier: row.identifier,
    memberRole: row.member_role,
    archived: row.archived_at != null,
  }));
}

/** The cycle/module a form opened from that context starts under. */
export function quickActionCreateContext(
  cycleId: string | null | undefined,
  moduleId: string | null | undefined,
  open: DialogOpen
): { cycleId?: string; moduleId?: string } | undefined {
  if (open !== "copy" && open !== "edit") return undefined;
  const context: { cycleId?: string; moduleId?: string } = {};
  if (cycleId != null && cycleId !== "") context.cycleId = cycleId;
  if (moduleId != null && moduleId !== "") context.moduleId = moduleId;
  return Object.keys(context).length === 0 ? undefined : context;
}

/** Row to the target shape a menu needs. */
export function quickActionTarget(row: IssueListItem, projectIdentifier: string): QuickActionTarget {
  return {
    id: row.id,
    name: row.name,
    sequenceId: row.sequence_id,
    projectId: row.project_id,
    projectIdentifier,
    stateId: row.state_id,
    snapshot: row,
    createdBy: row.created_by,
    archived: row.archived_at !== null,
    isEpic: row.is_epic === true,
  };
}
