/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useState } from "react";
import { observer } from "mobx-react";
import { MessageSquarePlus } from "lucide-react";
import { Button } from "@pi-dash/propel/button";
import type { TIssueServiceType } from "@pi-dash/types";
// local
import { CommentAndRunModal } from "./comment-and-run-modal";

type Props = {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  issueServiceType: TIssueServiceType;
  disabled?: boolean;
};

export const CommentAndRunActionButton = observer(function CommentAndRunActionButton(props: Props) {
  const { workspaceSlug, projectId, issueId, issueServiceType, disabled = false } = props;
  const [isOpen, setIsOpen] = useState(false);

  const handleClick = (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsOpen(true);
  };

  return (
    <>
      <Button variant="primary" size="lg" onClick={handleClick} disabled={disabled}>
        <MessageSquarePlus className="h-3.5 w-3.5 flex-shrink-0" strokeWidth={2} />
        <span className="text-body-xs-medium">Comment & Run</span>
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
