/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useEffect, useRef, useState } from "react";
import { observer } from "mobx-react";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { TIssueServiceType } from "@pi-dash/types";
import { ModalCore, TextArea } from "@pi-dash/ui";
// hooks
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
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
  const { t } = useTranslation();
  const { createComment } = useIssueDetail(issueServiceType);
  const [comment, setComment] = useState("");
  const [isPosting, setIsPosting] = useState(false);
  const { triggerRun, isSubmitting: isRunning } = useCreateAgentRun();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // Tracks whether the comment for this modal session has already been posted.
  // Prevents a duplicate post if the user retries after the agent-run step
  // failed (the comment write succeeded, only the run dispatch failed).
  const commentPostedRef = useRef(false);

  useEffect(() => {
    if (!isOpen) {
      setComment("");
      commentPostedRef.current = false;
      return;
    }
    // Focus the textarea after the modal mounts. Programmatic focus avoids the
    // jsx-a11y/no-autofocus rule while still landing the cursor where the user
    // is going to type.
    const handle = window.setTimeout(() => textareaRef.current?.focus(), 0);
    return () => window.clearTimeout(handle);
  }, [isOpen]);

  const isBusy = isPosting || isRunning;
  const trimmed = comment.trim();
  const canSubmit = trimmed.length > 0 && !isBusy;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;

    if (!commentPostedRef.current) {
      setIsPosting(true);
      try {
        await createComment(workspaceSlug, projectId, issueId, {
          comment_html: buildCommentHtml(trimmed),
        });
        commentPostedRef.current = true;
      } catch (error: unknown) {
        const message = (error as { error?: string })?.error ?? t("run_ai.comment_failed_message");
        setToast({
          type: TOAST_TYPE.ERROR,
          title: t("run_ai.comment_failed_title"),
          message,
        });
        setIsPosting(false);
        return;
      }
      setIsPosting(false);
    }

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
          <h3 className="text-h4-medium text-secondary">{t("run_ai.modal_title")}</h3>
          <p className="text-body-sm-regular text-tertiary">{t("run_ai.modal_description")}</p>
          <TextArea
            ref={textareaRef}
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder={t("run_ai.placeholder")}
            textAreaSize="md"
            rows={5}
          />
        </div>
        <div className="flex items-center justify-end gap-2 border-t-[0.5px] border-subtle px-5 py-4">
          <Button variant="secondary" size="lg" onClick={onClose} disabled={isBusy} type="button">
            {t("common.cancel")}
          </Button>
          <Button variant="primary" size="lg" type="submit" loading={isBusy} disabled={!canSubmit}>
            {isPosting ? t("run_ai.posting") : isRunning ? t("run_ai.starting") : t("run_ai.comment_button")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
