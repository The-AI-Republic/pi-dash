// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board settings behind the publish dialog
// (parity/specs/project-settings-lifecycle.md §PSET-016). Reads the
// project's board, publishes it with the chosen layouts and interaction
// flags, updates a dirty form, and unpublishes the board separately.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createProjectDeployBoard,
  deleteProjectDeployBoard,
  getProjectDeployBoard,
  updateProjectDeployBoard,
  type ProjectDeployBoard,
  type ProjectDeployBoardCreateInput,
  type ProjectDeployBoardStatus,
  type ProjectDeployBoardUpdateInput,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

export const PUBLISH_FAILED_TITLE = "Couldn't publish the project";
export const PUBLISHED_TITLE = "Project published";
export const PUBLISH_UPDATED_TITLE = "Publish settings saved";
export const UNPUBLISH_FAILED_TITLE = "Couldn't unpublish the project";
export const UNPUBLISHED_TITLE = "Project unpublished";

export const publishKeys = {
  board: (workspaceSlug: string, projectId: string) => ["project-publish", workspaceSlug, projectId] as const,
};

export function boardQueryOptions(workspaceSlug: string, projectId: string) {
  return {
    queryKey: publishKeys.board(workspaceSlug, projectId),
    queryFn: () => getProjectDeployBoard(getAppClient(), workspaceSlug, projectId),
  };
}

export function useDeployBoard(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...boardQueryOptions(workspaceSlug, projectId), enabled });
}

export interface PublishInput {
  workspaceSlug: string;
  projectId: string;
  input: ProjectDeployBoardCreateInput;
}

export function usePublishBoard() {
  const queryClient = useQueryClient();
  return useMutation<ProjectDeployBoard, Error, PublishInput>({
    mutationFn: async ({ workspaceSlug, projectId, input }) => {
      try {
        const board = await createProjectDeployBoard(getAppClient(), workspaceSlug, projectId, input);
        notifySuccess(PUBLISHED_TITLE, "The live link now opens the board.");
        return board;
      } catch (error) {
        notifyError(PUBLISH_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onSettled: (_result, _error, variables) => {
      void queryClient.invalidateQueries({ queryKey: publishKeys.board(variables.workspaceSlug, variables.projectId) });
    },
  });
}

export interface UpdatePublishInput {
  workspaceSlug: string;
  projectId: string;
  boardId: string;
  input: ProjectDeployBoardUpdateInput;
}

export function useUpdateBoard() {
  const queryClient = useQueryClient();
  return useMutation<ProjectDeployBoard, Error, UpdatePublishInput>({
    mutationFn: async ({ workspaceSlug, projectId, boardId, input }) => {
      try {
        const board = await updateProjectDeployBoard(getAppClient(), workspaceSlug, projectId, boardId, input);
        notifySuccess(PUBLISH_UPDATED_TITLE, "The live board now matches these settings.");
        return board;
      } catch (error) {
        notifyError(PUBLISH_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onSettled: (_result, _error, variables) => {
      void queryClient.invalidateQueries({ queryKey: publishKeys.board(variables.workspaceSlug, variables.projectId) });
    },
  });
}

export interface UnpublishInput {
  workspaceSlug: string;
  projectId: string;
  boardId: string;
}

export function useUnpublishBoard() {
  const queryClient = useQueryClient();
  return useMutation<number, Error, UnpublishInput>({
    mutationFn: async ({ workspaceSlug, projectId, boardId }) => {
      try {
        const status = await deleteProjectDeployBoard(getAppClient(), workspaceSlug, projectId, boardId);
        notifySuccess(UNPUBLISHED_TITLE, "The public board has been switched off.");
        return status;
      } catch (error) {
        notifyError(UNPUBLISH_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onSettled: (_result, _error, variables) => {
      void queryClient.invalidateQueries({ queryKey: publishKeys.board(variables.workspaceSlug, variables.projectId) });
    },
  });
}

/** True once a stored board exists and carries its public anchor. */
export function isBoardPublished(board: ProjectDeployBoardStatus | undefined): boolean {
  return (
    board !== undefined && "id" in board && board.id !== undefined && (board as ProjectDeployBoard).anchor !== null
  );
}

export interface BoardForm {
  list: boolean;
  kanban: boolean;
  comments: boolean;
  reactions: boolean;
  voting: boolean;
}

/** Initial form values: both layouts on, every interaction toggle off. */
export function boardFormFrom(board: ProjectDeployBoardStatus | undefined): BoardForm {
  if (board === undefined) {
    return { list: true, kanban: true, comments: false, reactions: false, voting: false };
  }
  return {
    list: board.view_props?.["list"] === true || board.view_props === null,
    kanban: board.view_props?.["kanban"] === true || board.view_props === null,
    comments: board.is_comments_enabled,
    reactions: board.is_reactions_enabled,
    voting: board.is_votes_enabled,
  };
}

/** Whether the form differs from the stored row. */
export function boardFormDirty(form: BoardForm, board: ProjectDeployBoardStatus | undefined): boolean {
  const stored = boardFormFrom(board);
  return (
    form.list !== stored.list ||
    form.kanban !== stored.kanban ||
    form.comments !== stored.comments ||
    form.reactions !== stored.reactions ||
    form.voting !== stored.voting
  );
}

/**
 * The public address for a published board. The spaces app owns the
 * destination; this dialog only builds the outbound link from the same
 * base-path configuration the stack ships with.
 */
export function publishedBoardLink(anchor: string): string {
  const configured = (import.meta.env.VITE_SPACE_BASE_URL as string | undefined) ?? "";
  const base = configured.trim() === "" ? globalThis.location.origin : configured;
  const path = (import.meta.env.VITE_SPACE_BASE_PATH as string | undefined) ?? "";
  return `${base}${path}/issues/${anchor}`;
}
