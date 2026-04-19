/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React, { useMemo } from "react";
import { observer } from "mobx-react";
import { useTranslation } from "@pi-dash/i18n";
import type { TIssueServiceType } from "@pi-dash/types";
import { EIssueServiceType } from "@pi-dash/types";
import { CollapsibleButton } from "@pi-dash/ui";
// hooks
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
// Pi Dash-web
import { useTimeLineRelationOptions } from "@/pi-dash-web/components/relations";
// local imports
import { RelationActionButton } from "./quick-action-button";

type Props = {
  isOpen: boolean;
  issueId: string;
  disabled: boolean;
  issueServiceType?: TIssueServiceType;
};

export const RelationsCollapsibleTitle = observer(function RelationsCollapsibleTitle(props: Props) {
  const { isOpen, issueId, disabled, issueServiceType = EIssueServiceType.ISSUES } = props;
  const { t } = useTranslation();
  // store hook
  const {
    relation: { getRelationCountByIssueId },
  } = useIssueDetail(issueServiceType);

  const ISSUE_RELATION_OPTIONS = useTimeLineRelationOptions();
  // derived values
  const relationsCount = getRelationCountByIssueId(issueId, ISSUE_RELATION_OPTIONS);

  // indicator element
  const indicatorElement = useMemo(
    () => (
      <span className="flex items-center justify-center">
        <p className="text-14 !leading-3 text-tertiary">{relationsCount}</p>
      </span>
    ),
    [relationsCount]
  );

  return (
    <CollapsibleButton
      isOpen={isOpen}
      title={t("common.relations")}
      indicatorElement={indicatorElement}
      actionItemElement={
        !disabled && <RelationActionButton issueId={issueId} disabled={disabled} issueServiceType={issueServiceType} />
      }
    />
  );
});
