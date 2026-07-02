/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
import { observer } from "mobx-react";
import { MessageSquarePlus } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import type { TButtonSize } from "@pi-dash/propel/button";
import type { TIssueComment } from "@pi-dash/types";
// local
import { useCreateAgentRun } from "./use-create-agent-run";

type Props = {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Posts the comment currently typed in the composer and returns it (or
   * ``undefined`` when empty / on failure). Supplied by ``CommentCreate`` via
   * its ``extraToolbarActions`` render prop so this button reuses the user's
   * existing input instead of opening a second text field. */
  submitComment: () => Promise<Partial<TIssueComment> | undefined>;
  disabled?: boolean;
  /** Render size; defaults to "lg" so the historical placement (top action
   * row) is unchanged. Pass "sm" or "base" when embedding in the comment
   * toolbar so the button matches the submit button's footprint. */
  size?: TButtonSize;
  /** Optional class overrides for parity with the surrounding submit
   * button (e.g. text size, padding). */
  className?: string;
};

export const CommentAndRunActionButton = observer(function CommentAndRunActionButton(props: Props) {
  const { workspaceSlug, projectId, issueId, submitComment, disabled = false, size = "lg", className } = props;
  const { t } = useTranslation();
  const { triggerRun, isSubmitting: isRunning } = useCreateAgentRun();

  const handleClick = async (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    // The comment the user typed in the composer IS the prompt — post it, then
    // immediately dispatch the run. No second dialog.
    const comment = await submitComment();
    if (!comment) return;
    await triggerRun({ workspaceSlug, projectId, issueId, mode: "comment_and_run" });
  };

  return (
    <Button
      variant="primary"
      size={size}
      onClick={handleClick}
      disabled={disabled || isRunning}
      loading={isRunning}
      className={className}
    >
      <MessageSquarePlus className="h-3.5 w-3.5 flex-shrink-0" strokeWidth={2} />
      <span className="text-body-xs-medium">{t("Comment & Run")}</span>
    </Button>
  );
});
