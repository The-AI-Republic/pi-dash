/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
import { observer } from "mobx-react";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { AiIcon } from "@pi-dash/propel/icons";
import { Tooltip } from "@pi-dash/propel/tooltip";
// hooks
import { usePlatformOS } from "@/hooks/use-platform-os";
// local
import { useCreateAgentRun } from "./use-create-agent-run";

type Props = {
  workspaceSlug: string;
  issueId: string;
  disabled?: boolean;
};

export const RunAIActionButton = observer(function RunAIActionButton(props: Props) {
  const { workspaceSlug, issueId, disabled = false } = props;
  const { t } = useTranslation();
  const { isMobile } = usePlatformOS();
  const { triggerRun, isSubmitting } = useCreateAgentRun();

  const handleClick = (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    triggerRun({ workspaceSlug, issueId, mode: "run_ai" });
  };

  return (
    <Tooltip
      tooltipContent={t(
        "Manually trigger an extra AI agent run. (Issues in the In Progress state already tick an agent run every few hours.)"
      )}
      isMobile={isMobile}
    >
      <span className="inline-flex">
        <Button
          variant="primary"
          size="lg"
          onClick={handleClick}
          disabled={disabled || isSubmitting}
          loading={isSubmitting}
        >
          <AiIcon className="h-3.5 w-3.5 flex-shrink-0" />
          <span className="text-body-xs-medium">{t("Manually Run AI")}</span>
        </Button>
      </span>
    </Tooltip>
  );
});
