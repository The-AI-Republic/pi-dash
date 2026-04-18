/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
// apple pi dash imports
import { useTranslation } from "@apple-pi-dash/i18n";
import { CycleIcon } from "@apple-pi-dash/propel/icons";
import { cn } from "@apple-pi-dash/utils";
// hooks
import { useCycle } from "@/hooks/store/use-cycle";

export type TReadonlyCycleProps = {
  className?: string;
  hideIcon?: boolean;
  value: string | null;
  placeholder?: string;
  projectId: string | undefined;
  workspaceSlug: string;
};

export const ReadonlyCycle = observer(function ReadonlyCycle(props: TReadonlyCycleProps) {
  const { className, hideIcon = false, value, placeholder, projectId, workspaceSlug } = props;

  const { t } = useTranslation();
  const { getCycleNameById, fetchAllCycles } = useCycle();
  const cycleName = value ? getCycleNameById(value) : null;

  useEffect(() => {
    if (projectId) {
      fetchAllCycles(workspaceSlug, projectId);
    }
  }, [projectId, workspaceSlug]);

  return (
    <div className={cn("flex items-center gap-1 text-13", className)}>
      {!hideIcon && <CycleIcon className="size-4 flex-shrink-0" />}
      <span className="flex-grow truncate">{cycleName ?? placeholder ?? t("common.none")}</span>
    </div>
  );
});
