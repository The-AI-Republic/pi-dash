/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useEffect, useState } from "react";
import { observer } from "mobx-react";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { TIssueServiceType } from "@pi-dash/types";
import { ModalCore, TextArea } from "@pi-dash/ui";
// services
import { IssueCommentService } from "@/services/issue";
// local
import { useCreateAgentRun } from "./use-create-agent-run";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  issueServiceType: TIssueServiceType;
};

function buildCommentHtml(plainText: string): string {
  const escaped = plainText.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return `<p>${escaped.replace(/\n/g, "<br />")}</p>`;
}

export const CommentAndRunModal = observer(function CommentAndRunModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, issueId, issueServiceType } = props;
  const [comment, setComment] = useState("");
  const [isPosting, setIsPosting] = useState(false);
  const { triggerRun, isSubmitting: isRunning } = useCreateAgentRun();

  useEffect(() => {
    if (!isOpen) setComment("");
  }, [isOpen]);

  const isBusy = isPosting || isRunning;
  const trimmed = comment.trim();
  const canSubmit = trimmed.length > 0 && !isBusy;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;

    const commentService = new IssueCommentService(issueServiceType);

    setIsPosting(true);
    try {
      await commentService.createIssueComment(workspaceSlug, projectId, issueId, {
        comment_html: buildCommentHtml(trimmed),
      });
    } catch (error: unknown) {
      const message = (error as { error?: string })?.error ?? "Failed to post the comment.";
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Could not post comment",
        message,
      });
      setIsPosting(false);
      return;
    }
    setIsPosting(false);

    const run = await triggerRun({
      workspaceSlug,
      issueId,
      prompt: trimmed,
    });
    if (run) onClose();
  };

  return (
    <ModalCore isOpen={isOpen} handleClose={isBusy ? () => {} : onClose}>
      <form onSubmit={handleSubmit}>
        <div className="space-y-4 p-5">
          <h3 className="text-h4-medium text-secondary">Comment & Run</h3>
          <p className="text-body-sm-regular text-tertiary">
            Post a comment on this work item and start an AI agent run with the comment as the prompt.
          </p>
          <TextArea
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder="Tell the agent what you want it to do..."
            textAreaSize="md"
            rows={5}
          />
        </div>
        <div className="flex items-center justify-end gap-2 border-t-[0.5px] border-subtle px-5 py-4">
          <Button variant="secondary" size="lg" onClick={onClose} disabled={isBusy} type="button">
            Cancel
          </Button>
          <Button variant="primary" size="lg" type="submit" loading={isBusy} disabled={!canSubmit}>
            {isPosting ? "Posting..." : isRunning ? "Starting run..." : "Comment & Run"}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
