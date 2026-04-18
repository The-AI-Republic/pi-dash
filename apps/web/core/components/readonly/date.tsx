/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// apple pi dash imports
import { useTranslation } from "@apple-pi-dash/i18n";
import { CalendarLayoutIcon } from "@apple-pi-dash/propel/icons";
import { cn, renderFormattedDate, getDate } from "@apple-pi-dash/utils";

export type TReadonlyDateProps = {
  className?: string;
  hideIcon?: boolean;
  value: Date | string | null;
  placeholder?: string;
  formatToken?: string;
};

export const ReadonlyDate = observer(function ReadonlyDate(props: TReadonlyDateProps) {
  const { className, hideIcon = false, value, placeholder, formatToken } = props;

  const { t } = useTranslation();
  const formattedDate = value ? renderFormattedDate(getDate(value), formatToken) : null;

  return (
    <div className={cn("flex items-center gap-1 text-13", className)}>
      {!hideIcon && <CalendarLayoutIcon className="size-4 flex-shrink-0" />}
      <span className="flex-grow truncate">{formattedDate ?? placeholder ?? t("common.none")}</span>
    </div>
  );
});
