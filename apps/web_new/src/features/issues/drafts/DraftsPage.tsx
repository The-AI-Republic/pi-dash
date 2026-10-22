// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace drafts screen (parity/specs/issues.md §12, ISS-137–141). The
// page reads one cursor-paged list of the caller's own drafts, renders a
// block per row, and owns the four row flows: edit (the shared work-item
// dialog opened on the draft's snapshot), copy (the same dialog on a
// duplicated payload), move to project (the dialog in move mode, whose
// "Add to project" promotes the draft), and delete behind a confirmation
// gated to the creator or a workspace admin (drafts spec §4.6,
// DRAFT-011); row abilities follow the DRAFT-025 matrix. No drafts
// invites the first one; no joined project invites a project instead.

import { Button, Skeleton } from "@pidash/kit";
import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, RefreshCw } from "lucide-react";
import * as React from "react";

import type { DraftIssue, DraftIssueCreateInput, ProjectDetails } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { useMe, usePermissions } from "../../../core/session/index.js";
import { projectDetailsQueryOptions } from "../../projects/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import type { SavedDraftRef, WorkItemCreateProject, WorkItemEditTarget } from "../components/WorkItemCreateDialog.js";
import { selectCreatableProjects } from "../create/createForm.js";
import {
  DRAFT_DELETE_FAILED_BODY,
  DRAFT_DELETE_FAILED_TITLE,
  DRAFT_DELETE_PERMISSION_BODY,
  DRAFT_DELETE_PERMISSION_TITLE,
  DRAFT_PROMOTE_FAILED_BODY,
  DRAFT_PROMOTE_FAILED_TITLE,
  DRAFT_PROMOTE_PERMISSION_BODY,
  DRAFT_PROMOTE_PERMISSION_TITLE,
  draftKeys,
  draftListQueryOptions,
  draftTotalCount,
  flattenDraftPages,
  isDraftPermissionRefusal,
  useCreateDraft,
  useDeleteDraft,
  usePromoteDraft,
  useUpdateDraftField,
} from "./api.js";
import { DraftBlock } from "./DraftBlock.js";
import { DraftsEmptyState, DraftsNoProjectState } from "./DraftEmptyStates.js";
import { DeleteDraftDialog } from "./DeleteDraftDialog.js";
import {
  copyDraftInput,
  copyDraftTitle,
  draftAbilities,
  draftSnapshot,
  type DraftFieldPatch,
  type DraftViewer,
} from "./model.js";

/** Page heading and create-entry label. */
export const DRAFTS_TITLE = "Drafts";
export const DRAFT_A_WORK_ITEM = "Draft a work item";
export const LOAD_MORE_LABEL = "Load more";

/** Props the route hands the page. */
export interface DraftsPageProps {
  workspaceSlug: string;
  /** The shared create/edit dialog, lazy-loaded by the route. */
  CreateDialog: React.ComponentType<DraftDialogProps>;
  /** Opens the workspace's create-project dialog (ISS-141). */
  onCreateProject: () => void;
}

/** Props the page hands the work-item dialog for one draft flow. */
export interface DraftDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  projects: WorkItemCreateProject[];
  edit?: WorkItemEditTarget | null;
  /** Seed values for a copy: the duplicated draft payload. */
  draftSeed?: DraftSeed | null;
  /** Move mode: the footer offers "Add to project". */
  moveToProject?: boolean;
  /** False for a guest-owned draft: edit is allowed, conversion is not. */
  canPublish?: boolean;
  onPublish?: (input: DraftIssueCreateInput) => Promise<void>;
  onCopySaved?: (input: DraftIssueCreateInput) => Promise<SavedDraftRef>;
  initialProjectId?: string;
}

/** The duplicated payload a copy opens with. */
export interface DraftSeed {
  title: string;
  input: DraftIssueCreateInput;
}

type OpenDialog =
  | { kind: "closed" }
  | { kind: "create" }
  | { kind: "edit"; draft: DraftIssue }
  | { kind: "copy"; draft: DraftIssue }
  | { kind: "move"; draft: DraftIssue };

/** The short code a draft shows; null when it holds no project. */
export function identifierOf(projects: readonly ProjectDetails[], projectId: string | null): string | null {
  if (projectId === null) return null;
  return projects.find((project) => project.id === projectId)?.identifier ?? null;
}

export function DraftsPage({ workspaceSlug, CreateDialog, onCreateProject }: DraftsPageProps): React.ReactElement {
  const client = getAppClient();
  const queryClient = useQueryClient();
  const permissions = usePermissions(workspaceSlug);
  const me = useMe();
  const drafts = useInfiniteQuery(draftListQueryOptions(client, workspaceSlug));
  const projectDetails = useQuery(projectDetailsQueryOptions(client, workspaceSlug));
  const create = useCreateDraft(workspaceSlug);
  const fieldWrite = useUpdateDraftField(workspaceSlug);
  const remove = useDeleteDraft(workspaceSlug);
  const promote = usePromoteDraft(workspaceSlug);
  const [dialog, setDialog] = React.useState<OpenDialog>({ kind: "closed" });
  const [pendingDelete, setPendingDelete] = React.useState<DraftIssue | null>(null);

  const rows = flattenDraftPages(drafts.data);
  const total = draftTotalCount(drafts.data);
  const projects = projectDetails.data ?? [];
  const viewer = {
    meId: me.data?.id ?? null,
    isAdmin: permissions.isAdmin,
    canWrite: permissions.canEdit,
  };
  const joined = projects.filter((project) => project.member_role !== null);
  // "No project" follows the joined projects the viewer can create work
  // in, as the old root does: the empty screen names creating or joining
  // one. Archived projects never count (ISS-141).
  const hasProject = joined.some((project) => project.archived_at === null);
  const creatable: WorkItemCreateProject[] = React.useMemo(
    () =>
      selectCreatableProjects(
        projects.map((project) => ({
          id: project.id,
          name: project.name,
          identifier: project.identifier,
          memberRole: project.member_role,
          archived: project.archived_at !== null,
        }))
      ),
    [projects]
  );

  function closeDialog(): void {
    setDialog({ kind: "closed" });
  }

  // A modal edit writes through the shared edit path, which refreshes the
  // project issue lists but not this page's cache; re-read so the block
  // shows the saved values (ISS-139).
  function refreshAfterEdit(): void {
    closeDialog();
    void queryClient.invalidateQueries({ queryKey: draftKeys.list(workspaceSlug) });
  }

  function writeField(draft: DraftIssue, patch: DraftFieldPatch): void {
    fieldWrite.mutate({ draftId: draft.id, patch });
  }

  function requestDelete(draft: DraftIssue): void {
    // The matrix's manage bit is the creator-or-workspace-admin delete
    // standing (DRAFT-011).
    if (!draftAbilities(draft, viewer).manage) {
      notifyError(DRAFT_DELETE_PERMISSION_TITLE, DRAFT_DELETE_PERMISSION_BODY);
      return;
    }
    setPendingDelete(draft);
  }

  function confirmDelete(draftId: string): void {
    remove.mutate(draftId, {
      onError: (error) => {
        if (isDraftPermissionRefusal(error)) {
          notifyError(DRAFT_DELETE_PERMISSION_TITLE, DRAFT_DELETE_PERMISSION_BODY);
        } else {
          notifyError(DRAFT_DELETE_FAILED_TITLE, DRAFT_DELETE_FAILED_BODY);
        }
      },
      onSettled: () => {
        setPendingDelete(null);
      },
    });
  }

  const loading = (): React.ReactElement => (
    <div data-testid="drafts-loading" className="flex flex-col gap-(--space-2) p-(--space-4)">
      {Array.from({ length: 6 }, (_, index) => (
        <Skeleton key={index} className="h-14 w-full" />
      ))}
    </div>
  );

  const body = ((): React.ReactElement => {
    // Drafts paint before their projects do. Until that list lands a row
    // has no code and no pickers, and "you have no project" is not yet
    // known, so a returning viewer waits rather than sees either.
    if (drafts.isPending || (rows.length > 0 && projectDetails.isPending)) return loading();
    if (drafts.isError) {
      return (
        <div className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
          <h2 className="text-h3 text-(--text)">Couldn't load drafts</h2>
          <p className="text-body text-(--text-muted)">Check your connection and try again.</p>
          <Button variant="secondary" onClick={() => void drafts.refetch()}>
            <RefreshCw size={14} strokeWidth={1.5} aria-hidden="true" />
            Try again
          </Button>
        </div>
      );
    }
    if (!hasProject) {
      return <DraftsNoProjectState canCreate={permissions.canEdit} onCreateProject={onCreateProject} />;
    }
    if (rows.length === 0) {
      return <DraftsEmptyState canCreate={permissions.canEdit} onCreate={() => setDialog({ kind: "create" })} />;
    }
    return (
      <div data-testid="drafts-list" className="relative">
        {rows.map((draft) => (
          <DraftBlock
            key={draft.id}
            workspaceSlug={workspaceSlug}
            draft={draft}
            project={projects.find((project) => project.id === draft.project_id)}
            projectIdentifier={identifierOf(projects, draft.project_id)}
            abilities={draftAbilities(draft, viewer)}
            onEdit={() => setDialog({ kind: "edit", draft })}
            onCopy={() => setDialog({ kind: "copy", draft })}
            onMove={() => setDialog({ kind: "move", draft })}
            onDelete={() => requestDelete(draft)}
            onWrite={(patch) => writeField(draft, patch)}
          />
        ))}
        {drafts.hasNextPage ? (
          drafts.isFetchingNextPage ? (
            <div data-testid="drafts-load-more-loading" className="p-(--space-3)">
              <Skeleton className="h-8 w-full" />
            </div>
          ) : (
            <button
              type="button"
              onClick={() => drafts.fetchNextPage()}
              className="text-body w-full border-b border-(--border) bg-(--surface) p-(--space-3) pl-(--space-6) text-left font-medium text-(--accent) hover:underline focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
            >
              {LOAD_MORE_LABEL} &darr;
            </button>
          )
        ) : null}
      </div>
    );
  })();

  return (
    <section aria-label="Drafts" className="relative flex h-full flex-col">
      <div className="flex items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3)">
        <h1 className="text-h3 text-(--text)">{DRAFTS_TITLE}</h1>
        {total > 0 ? (
          <span
            data-testid="drafts-count"
            className="text-caption rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
          >
            {total}
          </span>
        ) : null}
        {hasProject ? (
          <Button
            variant="primary"
            size="small"
            className="ml-auto"
            disabled={!permissions.canEdit}
            onClick={() => setDialog({ kind: "create" })}
          >
            <Plus size={14} strokeWidth={2} aria-hidden="true" />
            {DRAFT_A_WORK_ITEM}
          </Button>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">{body}</div>
      <DraftDialogHost
        dialog={dialog}
        workspaceSlug={workspaceSlug}
        creatable={creatable}
        viewer={viewer}
        CreateDialog={CreateDialog}
        onClose={closeDialog}
        onCopySaved={(input) => create.mutateAsync(input)}
        onPromote={async (draftId, input) => {
          try {
            await promote.mutateAsync({ draftId, input });
            closeDialog();
          } catch (error) {
            if (isDraftPermissionRefusal(error)) {
              notifyError(DRAFT_PROMOTE_PERMISSION_TITLE, DRAFT_PROMOTE_PERMISSION_BODY);
            } else {
              notifyError(DRAFT_PROMOTE_FAILED_TITLE, DRAFT_PROMOTE_FAILED_BODY);
            }
            throw error;
          }
        }}
        onEditChanged={refreshAfterEdit}
      />
      <DeleteDraftDialog
        open={pendingDelete !== null}
        draftName={pendingDelete?.name ?? ""}
        deleting={remove.isPending}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (pendingDelete !== null) confirmDelete(pendingDelete.id);
        }}
      />
    </section>
  );
}

function DraftDialogHost({
  dialog,
  workspaceSlug,
  creatable,
  viewer,
  CreateDialog,
  onClose,
  onCopySaved,
  onPromote,
  onEditChanged,
}: {
  dialog: OpenDialog;
  workspaceSlug: string;
  creatable: WorkItemCreateProject[];
  viewer: DraftViewer;
  CreateDialog: React.ComponentType<DraftDialogProps>;
  onClose: () => void;
  onCopySaved: (input: DraftIssueCreateInput) => Promise<SavedDraftRef>;
  onPromote: (draftId: string, input: DraftIssueCreateInput) => Promise<void>;
  onEditChanged: () => void;
}): React.ReactElement | null {
  if (dialog.kind === "closed") return null;
  const shared = {
    open: true,
    onOpenChange: (open: boolean) => {
      if (!open) onClose();
    },
    workspaceSlug,
    projects: creatable,
    canPublish: dialog.kind === "create" ? true : draftAbilities(dialog.draft, viewer).convert,
  };
  if (dialog.kind === "create") {
    return <CreateDialog {...shared} onCopySaved={onCopySaved} />;
  }
  const draft = dialog.draft;
  const edit: WorkItemEditTarget = {
    kind: "draft",
    id: draft.id,
    projectId: draft.project_id,
    snapshot: draftSnapshot(draft),
    onChanged: onEditChanged,
  };
  if (dialog.kind === "edit") {
    return <CreateDialog {...shared} edit={edit} />;
  }
  if (dialog.kind === "copy") {
    return (
      <CreateDialog
        {...shared}
        draftSeed={{ title: copyDraftTitle(draft), input: copyDraftInput(draft) }}
        onCopySaved={onCopySaved}
      />
    );
  }
  return <CreateDialog {...shared} edit={edit} moveToProject onPublish={(input) => onPromote(draft.id, input)} />;
}
