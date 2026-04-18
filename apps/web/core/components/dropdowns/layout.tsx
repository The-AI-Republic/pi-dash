/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useMemo } from "react";
import { observer } from "mobx-react";
// apple pi dash imports
import { ISSUE_LAYOUT_MAP } from "@apple-pi-dash/constants";
import { useTranslation } from "@apple-pi-dash/i18n";
import { CheckIcon } from "@apple-pi-dash/propel/icons";
import { EIssueLayoutTypes } from "@apple-pi-dash/types";
import { Dropdown } from "@apple-pi-dash/ui";
import { cn } from "@apple-pi-dash/utils";
// components
import { IssueLayoutIcon } from "@/components/issues/issue-layouts/layout-icon";
import { getIconButtonStyling } from "@apple-pi-dash/propel/icon-button";

type TLayoutDropDown = {
  onChange: (value: EIssueLayoutTypes) => void;
  value: EIssueLayoutTypes;
  disabledLayouts?: EIssueLayoutTypes[];
};

export const LayoutDropDown = observer(function LayoutDropDown(props: TLayoutDropDown) {
  const { onChange, value = EIssueLayoutTypes.LIST, disabledLayouts = [] } = props;
  // apple pi dash i18n
  const { t } = useTranslation();
  // derived values
  const availableLayouts = useMemo(
    () => Object.values(ISSUE_LAYOUT_MAP).filter((layout) => !disabledLayouts.includes(layout.key)),
    [disabledLayouts]
  );

  const options = useMemo(
    () =>
      availableLayouts.map((issueLayout) => ({
        data: issueLayout.key,
        value: issueLayout.key,
      })),
    [availableLayouts]
  );

  const buttonContent = useCallback((isOpen: boolean, buttonValue: string | string[] | undefined) => {
    const dropdownValue = ISSUE_LAYOUT_MAP[buttonValue as EIssueLayoutTypes];
    return (
      <div className="flex items-center gap-2 text-secondary">
        <IssueLayoutIcon layout={dropdownValue.key} strokeWidth={2} className={`size-3.5 text-secondary`} />
        <span className="text-11 font-medium">{t(dropdownValue.i18n_label)}</span>
      </div>
    );
  }, []);

  const itemContent = useCallback((props: { value: string; selected: boolean }) => {
    const dropdownValue = ISSUE_LAYOUT_MAP[props.value as EIssueLayoutTypes];

    return (
      <div className={cn("flex w-full items-center justify-between gap-2 text-secondary")}>
        <div className="flex items-center gap-2">
          <IssueLayoutIcon layout={dropdownValue.key} strokeWidth={2} className={`size-3 text-secondary`} />
          <span className="text-11 font-medium">{t(dropdownValue.i18n_label)}</span>
        </div>
        {props.selected && <CheckIcon className="h-3.5 w-3.5 flex-shrink-0" />}
      </div>
    );
  }, []);

  const keyExtractor = useCallback((option: any) => option.value, []);

  return (
    <Dropdown
      onChange={onChange as (value: string) => void}
      value={value?.toString()}
      keyExtractor={keyExtractor}
      options={options}
      buttonContainerClassName={cn(getIconButtonStyling("secondary", "lg"), "w-auto px-2")}
      buttonContent={buttonContent}
      renderItem={itemContent}
      disableSearch
    />
  );
});
