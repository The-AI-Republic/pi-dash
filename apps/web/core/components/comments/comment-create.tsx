/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useRef, useState } from "react";
import { observer } from "mobx-react";
import { useForm, Controller } from "react-hook-form";
// pi dash imports
import { EIssueCommentAccessSpecifier } from "@pi-dash/constants";
import type { EditorRefApi } from "@pi-dash/editor";
import type { TIssueComment, TCommentsOperations } from "@pi-dash/types";
import { cn, isCommentEmpty } from "@pi-dash/utils";
// components
import { LiteTextEditor } from "@/components/editor/lite-text";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";
// services
import { FileService } from "@/services/file.service";

type TCommentCreate = {
  entityId: string;
  workspaceSlug: string;
  activityOperations: TCommentsOperations;
  showToolbarInitially?: boolean;
  projectId?: string;
  onSubmitCallback?: (elementId: string) => void;
  /** Optional content rendered to the LEFT of the toolbar's submit button.
   * Forwarded to ``LiteTextEditor`` via ``extraToolbarActions``.
   *
   * May be a plain node, or a render function that receives live composer
   * state — ``isEmpty`` / ``isSubmitting`` and a ``submitComment`` callback
   * that posts whatever is currently typed (returning the created comment, or
   * ``undefined`` if empty / on failure). This lets a toolbar action (e.g.
   * "Comment & Run") reuse the comment the user already typed instead of
   * popping a second input. */
  extraToolbarActions?:
    | React.ReactNode
    | ((ctx: {
        isEmpty: boolean;
        isSubmitting: boolean;
        submitComment: () => Promise<Partial<TIssueComment> | undefined>;
      }) => React.ReactNode);
};

// services
const fileService = new FileService();

export const CommentCreate = observer(function CommentCreate(props: TCommentCreate) {
  const {
    workspaceSlug,
    entityId,
    activityOperations,
    showToolbarInitially = false,
    projectId,
    onSubmitCallback,
    extraToolbarActions,
  } = props;
  // states
  const [uploadedAssetIds, setUploadedAssetIds] = useState<string[]>([]);
  // Tracks an in-flight imperative post (``submitComment``). ``handleSubmit``
  // maintains its own ``isSubmitting``, but the imperative path bypasses it, so
  // we track posting separately and merge the two below into a single
  // ``isSubmitting`` that gates the editor and toolbar actions.
  const [isPosting, setIsPosting] = useState(false);
  // refs
  const editorRef = useRef<EditorRefApi>(null);
  // store hooks
  const workspaceStore = useWorkspace();
  // derived values
  const workspaceId = workspaceStore.getWorkspaceBySlug(workspaceSlug)?.id as string;
  // form info
  const {
    handleSubmit,
    control,
    watch,
    getValues,
    formState: { isSubmitting: formIsSubmitting },
    reset,
  } = useForm<Partial<TIssueComment>>({
    defaultValues: {
      comment_html: "<p></p>",
    },
  });

  // Unified busy flag: true while either the native form submit or an imperative
  // post is in flight. Prevents concurrent/duplicate submissions from a rapid
  // double-click on "Comment & Run" (which would otherwise post twice and
  // dispatch two agent runs).
  const isSubmitting = formIsSubmitting || isPosting;

  // Posts ``formData`` as a comment, syncs any uploaded assets, then resets the
  // editor. Returns the created comment so callers (e.g. Comment & Run) can act
  // on it. Returns ``undefined`` on failure; the editor is reset either way to
  // match the historical submit behavior.
  const postComment = useCallback(
    async (formData: Partial<TIssueComment>): Promise<Partial<TIssueComment> | undefined> => {
      setIsPosting(true);
      try {
        const comment = await activityOperations.createComment(formData);
        if (comment?.id) onSubmitCallback?.(comment.id);
        if (uploadedAssetIds.length > 0) {
          if (projectId) {
            await fileService.updateBulkProjectAssetsUploadStatus(workspaceSlug, projectId.toString(), entityId, {
              asset_ids: uploadedAssetIds,
            });
          } else {
            await fileService.updateBulkWorkspaceAssetsUploadStatus(workspaceSlug, entityId, {
              asset_ids: uploadedAssetIds,
            });
          }
          setUploadedAssetIds([]);
        }
        return comment;
      } catch (error) {
        console.error(error);
        return undefined;
      } finally {
        setIsPosting(false);
        reset({
          comment_html: "<p></p>",
        });
        editorRef.current?.clearEditor();
      }
    },
    [activityOperations, onSubmitCallback, uploadedAssetIds, projectId, workspaceSlug, entityId, reset]
  );

  const onSubmit = async (formData: Partial<TIssueComment>) => {
    await postComment(formData);
  };

  // Imperative submit for toolbar actions: posts whatever is currently typed
  // (no second input), guarding against empty content so an accidental click on
  // an empty composer is a no-op. Also no-ops while a post is already in flight.
  const submitComment = useCallback(async (): Promise<Partial<TIssueComment> | undefined> => {
    if (isSubmitting) return undefined;
    const formData = getValues();
    if (isCommentEmpty(formData.comment_html ?? undefined)) return undefined;
    return postComment(formData);
  }, [isSubmitting, getValues, postComment]);

  const commentHTML = watch("comment_html");
  const isEmpty = isCommentEmpty(commentHTML ?? undefined);

  const resolvedExtraToolbarActions =
    typeof extraToolbarActions === "function"
      ? extraToolbarActions({ isEmpty, isSubmitting, submitComment })
      : extraToolbarActions;

  return (
    <div
      role="group"
      aria-label="Add comment"
      className={cn("sticky bottom-0 z-[4] bg-surface-1 sm:static")}
      onKeyDown={(e) => {
        if (
          e.key === "Enter" &&
          !e.shiftKey &&
          !e.ctrlKey &&
          !e.metaKey &&
          !isEmpty &&
          !isSubmitting &&
          editorRef.current?.isEditorReadyToDiscard()
        )
          handleSubmit(onSubmit)(e);
      }}
    >
      <Controller
        name="access"
        control={control}
        render={({ field: { onChange: onAccessChange, value: accessValue } }) => (
          <Controller
            name="comment_html"
            control={control}
            render={({ field: { value, onChange } }) => (
              <LiteTextEditor
                editable
                workspaceId={workspaceId}
                id={"add_comment_" + entityId}
                value={"<p></p>"}
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                onEnterKeyPress={(e) => {
                  if (!isEmpty && !isSubmitting) {
                    handleSubmit(onSubmit)(e);
                  }
                }}
                ref={editorRef}
                initialValue={value ?? "<p></p>"}
                containerClassName="min-h-min"
                onChange={(comment_json, comment_html) => onChange(comment_html)}
                accessSpecifier={accessValue ?? EIssueCommentAccessSpecifier.INTERNAL}
                handleAccessChange={onAccessChange}
                isSubmitting={isSubmitting}
                uploadFile={async (blockId, file) => {
                  const { asset_id } = await activityOperations.uploadCommentAsset(blockId, file);
                  setUploadedAssetIds((prev) => [...prev, asset_id]);
                  return asset_id;
                }}
                duplicateFile={async (assetId: string) => {
                  const { asset_id } = await activityOperations.duplicateCommentAsset(assetId);
                  setUploadedAssetIds((prev) => [...prev, asset_id]);
                  return asset_id;
                }}
                showToolbarInitially={showToolbarInitially}
                parentClassName="p-2"
                displayConfig={{
                  fontSize: "small-font",
                }}
                extraToolbarActions={resolvedExtraToolbarActions}
              />
            )}
          />
        )}
      />
    </div>
  );
});
