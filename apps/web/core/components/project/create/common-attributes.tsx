/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ChangeEvent } from "react";
import type { UseFormSetValue } from "react-hook-form";
import { Controller, useFormContext } from "react-hook-form";
import { InfoIcon } from "@pi-dash/propel/icons";
// pi dash imports
import { ETabIndices } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
// ui
import { Tooltip } from "@pi-dash/propel/tooltip";
import { Input, TextArea } from "@pi-dash/ui";
import { cn, projectIdentifierSanitizer, getTabIndex } from "@pi-dash/utils";
// pi dash utils
// helpers
// pi-dash-web types
import type { TProject } from "@/pi-dash-web/types/projects";

type Props = {
  setValue: UseFormSetValue<TProject>;
  isMobile: boolean;
  shouldAutoSyncIdentifier: boolean;
  setShouldAutoSyncIdentifier: (value: boolean) => void;
  handleFormOnChange?: () => void;
};

function ProjectCommonAttributes(props: Props) {
  const { setValue, isMobile, shouldAutoSyncIdentifier, setShouldAutoSyncIdentifier, handleFormOnChange } = props;
  const {
    formState: { errors },
    control,
  } = useFormContext<TProject>();

  const { getIndex } = getTabIndex(ETabIndices.PROJECT_CREATE, isMobile);
  const { t } = useTranslation();

  const handleNameChange =
    (onChange: (event: ChangeEvent<HTMLInputElement>) => void) => (e: ChangeEvent<HTMLInputElement>) => {
      if (!shouldAutoSyncIdentifier) {
        onChange(e);
        return;
      }
      if (e.target.value === "") setValue("identifier", "");
      else setValue("identifier", projectIdentifierSanitizer(e.target.value).substring(0, 10));
      onChange(e);
      handleFormOnChange?.();
    };

  const handleIdentifierChange = (onChange: (value: string) => void) => (e: ChangeEvent<HTMLInputElement>) => {
    const { value } = e.target;
    const alphanumericValue = projectIdentifierSanitizer(value);
    setShouldAutoSyncIdentifier(false);
    onChange(alphanumericValue);
    handleFormOnChange?.();
  };
  return (
    <div className="grid grid-cols-1 gap-x-2 gap-y-3 md:grid-cols-4">
      <div className="md:col-span-3">
        <Controller
          control={control}
          name="name"
          rules={{
            required: t("name_is_required"),
            maxLength: {
              value: 255,
              message: t("title_should_be_less_than_255_characters"),
            },
          }}
          render={({ field: { value, onChange } }) => (
            <Input
              id="name"
              name="name"
              type="text"
              value={value}
              onChange={handleNameChange(onChange)}
              hasError={Boolean(errors.name)}
              placeholder={t("project_name")}
              className="focus:border-blue-400 w-full"
              tabIndex={getIndex("name")}
            />
          )}
        />
        <span className="text-11 text-danger-primary">{errors?.name?.message}</span>
      </div>
      <div className="relative">
        <Controller
          control={control}
          name="identifier"
          rules={{
            required: t("project_id_is_required"),
            // allow only alphanumeric & non-latin characters
            validate: (value) =>
              /^[ÇŞĞIİÖÜA-Z0-9]+$/.test(value.toUpperCase()) || t("only_alphanumeric_non_latin_characters_allowed"),
            minLength: {
              value: 1,
              message: t("project_id_min_char"),
            },
            maxLength: {
              value: 10,
              message: t("project_id_max_char"),
            },
          }}
          render={({ field: { value, onChange } }) => (
            <Input
              id="identifier"
              name="identifier"
              type="text"
              value={value}
              onChange={handleIdentifierChange(onChange)}
              hasError={Boolean(errors.identifier)}
              placeholder={t("project_id")}
              className={cn("focus:border-blue-400 w-full pr-7 text-11", {
                uppercase: value,
              })}
              tabIndex={getIndex("identifier")}
            />
          )}
        />
        <Tooltip
          isMobile={isMobile}
          tooltipContent={t("project_id_tooltip_content")}
          className="text-13"
          position="right-start"
        >
          <InfoIcon className="absolute top-2.5 right-2 h-3 w-3 text-placeholder" />
        </Tooltip>
        <span className="text-11 text-danger-primary">{errors?.identifier?.message}</span>
      </div>
      <div className="md:col-span-4">
        <Controller
          name="description"
          control={control}
          render={({ field: { value, onChange } }) => (
            <TextArea
              id="description"
              name="description"
              value={value}
              placeholder={t("description")}
              onChange={(e) => {
                onChange(e);
                handleFormOnChange?.();
              }}
              className="focus:border-blue-400 !h-24 text-13"
              hasError={Boolean(errors?.description)}
              tabIndex={getIndex("description")}
            />
          )}
        />
      </div>
      <div className="md:col-span-3">
        <Controller
          name="repo_url"
          control={control}
          rules={{
            maxLength: {
              value: 512,
              message: t("repo_url_too_long") || "Repository URL is too long",
            },
          }}
          render={({ field: { value, onChange } }) => (
            <Input
              id="repo_url"
              name="repo_url"
              type="text"
              value={value ?? ""}
              onChange={(e) => {
                onChange(e);
                handleFormOnChange?.();
              }}
              hasError={Boolean(errors?.repo_url)}
              placeholder={
                t("git_repository_url_placeholder") || "Git repository URL (e.g. git@github.com:org/repo.git)"
              }
              className="focus:border-blue-400 w-full"
              tabIndex={getIndex("repo_url")}
            />
          )}
        />
        <span className="text-11 text-danger-primary">{errors?.repo_url?.message}</span>
      </div>
      <div>
        <Controller
          name="base_branch"
          control={control}
          rules={{
            maxLength: {
              value: 128,
              message: t("base_branch_too_long") || "Base branch is too long",
            },
            pattern: {
              value: /^[A-Za-z0-9._/-]*$/,
              message: t("base_branch_invalid_chars") || "Only letters, numbers, and . _ / - are allowed",
            },
          }}
          render={({ field: { value, onChange } }) => (
            <Input
              id="base_branch"
              name="base_branch"
              type="text"
              value={value ?? ""}
              onChange={(e) => {
                onChange(e);
                handleFormOnChange?.();
              }}
              hasError={Boolean(errors?.base_branch)}
              placeholder={t("base_branch_placeholder") || "Base branch (leave empty to use remote default)"}
              className="focus:border-blue-400 w-full"
              tabIndex={getIndex("base_branch")}
            />
          )}
        />
        <span className="text-11 text-danger-primary">{errors?.base_branch?.message}</span>
      </div>
    </div>
  );
}

export default ProjectCommonAttributes;
