/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useState } from "react";
import { observer } from "mobx-react";
import { MessageSquarePlus } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import type { TButtonSize } from "@pi-dash/propel/button";
import type { TIssueServiceType } from "@pi-dash/types";
// local
import { CommentAndRunModal } from "./comment-and-run-modal";

type Props = {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  issueServiceType: TIssueServiceType;
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
  const { workspaceSlug, projectId, issueId, issueServiceType, disabled = false, size = "lg", className } = props;
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);

  const handleClick = (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsOpen(true);
  };

  return (
    <>
      <Button variant="primary" size={size} onClick={handleClick} disabled={disabled} className={className}>
        <MessageSquarePlus className="h-3.5 w-3.5 flex-shrink-0" strokeWidth={2} />
        <span className="text-body-xs-medium">{t("run_ai.comment_button")}</span>
      </Button>
      <CommentAndRunModal
        isOpen={isOpen}
        onClose={() => setIsOpen(false)}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issueId={issueId}
        issueServiceType={issueServiceType}
      />
    </>
  );
});
