/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
import { observer } from "mobx-react";
import type { Control, FormState } from "react-hook-form";
import { Controller } from "react-hook-form";
// pi dash imports
import { ETabIndices } from "@pi-dash/constants";
// types
import { useTranslation } from "@pi-dash/i18n";
import type { TIssue } from "@pi-dash/types";
// ui
import { Input } from "@pi-dash/ui";
// helpers
import { getTabIndex } from "@pi-dash/utils";
// hooks
import { usePlatformOS } from "@/hooks/use-platform-os";

type TIssueTitleInputProps = {
  control: Control<TIssue>;
  issueTitleRef: React.MutableRefObject<HTMLInputElement | null>;
  formState: FormState<TIssue>;
  handleFormChange: () => void;
  /**
   * When true, the user has a working Pi Dash AI assistant configured, so the
   * title becomes optional — if left blank, Pi Dash AI generates one from the
   * description on save. When false, the title stays required.
   */
  aiAvailable?: boolean;
};

export const IssueTitleInput = observer(function IssueTitleInput(props: TIssueTitleInputProps) {
  const {
    control,
    issueTitleRef,
    formState: { errors },
    handleFormChange,
    aiAvailable = false,
  } = props;
  // store hooks
  const { isMobile } = usePlatformOS();
  const { t } = useTranslation();

  const { getIndex } = getTabIndex(ETabIndices.ISSUE_FORM, isMobile);

  // When AI is available the title is optional (generated on save), so only the
  // length rule applies. Otherwise enforce the required + non-whitespace rules.
  const validateWhitespace = (value: string) => {
    if (aiAvailable) return undefined;
    if ((value ?? "").trim() === "") {
      return t("Title is required");
    }
    return undefined;
  };
  return (
    <div>
      <Controller
        control={control}
        name="name"
        rules={{
          validate: validateWhitespace,
          required: aiAvailable ? false : t("Title is required"),
          maxLength: {
            value: 255,
            message: t("Title should be less than 255 characters"),
          },
        }}
        render={({ field: { value, onChange, ref } }) => (
          <Input
            id="name"
            name="name"
            type="text"
            value={value}
            onChange={(e) => {
              onChange(e.target.value);
              handleFormChange();
            }}
            ref={issueTitleRef || ref}
            hasError={Boolean(errors.name)}
            placeholder={aiAvailable ? t("Title is optional — Pi Dash AI will generate it") : t("Title")}
            className="w-full text-body-sm-regular"
            // oxlint-disable-next-line jsx-a11y/no-autofocus -- The create/edit modal intentionally focuses the title field on open.
            autoFocus
            tabIndex={getIndex("name")}
          />
        )}
      />
      <span className="text-caption-sm-medium text-danger-primary">{errors?.name?.message}</span>
    </div>
  );
});
