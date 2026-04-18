/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// apple pi dash imports
import { ISSUE_PRIORITIES } from "@apple-pi-dash/constants";
import { useTranslation } from "@apple-pi-dash/i18n";
import { PriorityIcon } from "@apple-pi-dash/propel/icons";
import type { TIssuePriorities } from "@apple-pi-dash/types";
import { cn } from "@apple-pi-dash/utils";

export type TReadonlyPriorityProps = {
  className?: string;
  hideIcon?: boolean;
  value: TIssuePriorities | undefined | null;
  placeholder?: string;
};

export const ReadonlyPriority = observer(function ReadonlyPriority(props: TReadonlyPriorityProps) {
  const { className, hideIcon = false, value, placeholder } = props;

  const { t } = useTranslation();
  const priorityDetails = ISSUE_PRIORITIES.find((p) => p.key === value);

  return (
    <div className={cn("flex items-center gap-1 text-body-xs-regular", className)}>
      {!hideIcon && <PriorityIcon priority={value ?? "none"} size={12} className="flex-shrink-0" withContainer />}
      <span className="flex-grow truncate">{priorityDetails?.title ?? placeholder ?? t("common.none")}</span>
    </div>
  );
});
