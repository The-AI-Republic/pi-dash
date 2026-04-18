/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useRef, useState } from "react";
import { observer } from "mobx-react";
// apple pi dash imports
import type { EditorRefApi } from "@apple-pi-dash/editor";
import type { TIssueComment, TCommentsOperations } from "@apple-pi-dash/types";
// apple pi dash web imports
import { CommentBlock, CommentCardDisplay } from "@/apple-pi-dash-web/components/comments";
// local imports
import { CommentQuickActions } from "../quick-actions";

type TCommentCard = {
  workspaceSlug: string;
  entityId: string;
  comment: TIssueComment | undefined;
  activityOperations: TCommentsOperations;
  ends: "top" | "bottom" | undefined;
  showAccessSpecifier: boolean;
  showCopyLinkOption: boolean;
  enableReplies: boolean;
  disabled?: boolean;
  projectId?: string;
};

export const CommentCard = observer(function CommentCard(props: TCommentCard) {
  const {
    workspaceSlug,
    entityId,
    comment,
    activityOperations,
    ends,
    showAccessSpecifier,
    showCopyLinkOption,
    disabled = false,
    projectId,
  } = props;
  // states
  const [isEditing, setIsEditing] = useState(false);
  // refs
  const readOnlyEditorRef = useRef<EditorRefApi>(null);
  // derived values
  const workspaceId = comment?.workspace;

  if (!comment || !workspaceId) return null;

  return (
    <CommentBlock comment={comment} ends={ends}>
      <CommentCardDisplay
        activityOperations={activityOperations}
        entityId={entityId}
        comment={comment}
        disabled={disabled}
        projectId={projectId}
        readOnlyEditorRef={readOnlyEditorRef}
        showAccessSpecifier={showAccessSpecifier}
        workspaceId={workspaceId}
        workspaceSlug={workspaceSlug}
        isEditing={isEditing}
        setIsEditing={setIsEditing}
        renderQuickActions={() => (
          <CommentQuickActions
            activityOperations={activityOperations}
            comment={comment}
            setEditMode={() => setIsEditing(true)}
            showAccessSpecifier={showAccessSpecifier}
            showCopyLinkOption={showCopyLinkOption}
          />
        )}
      />
    </CommentBlock>
  );
});
