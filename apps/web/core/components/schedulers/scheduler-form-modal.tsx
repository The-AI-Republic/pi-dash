/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import type { IScheduler } from "@pi-dash/services";
import { EModalPosition, EModalWidth, Input, ModalCore, TextArea, ToggleSwitch } from "@pi-dash/ui";
import { SCHEDULER_COLOR_PALETTE as COLOR_PALETTE } from "@/components/project/scheduler-bindings/constants";

interface SchedulerFormValues {
  slug: string;
  name: string;
  description: string;
  prompt: string;
  color: string;
  is_enabled: boolean;
}

type Props = {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (values: SchedulerFormValues) => Promise<void>;
  /** When set, the form is in edit mode — slug is locked. */
  scheduler?: IScheduler | null;
};

const emptyValues: SchedulerFormValues = {
  slug: "",
  name: "",
  description: "",
  prompt: "",
  color: COLOR_PALETTE[0],
  is_enabled: true,
};

const SLUG_PATTERN = /^[a-z0-9][a-z0-9-]*$/;

export const SchedulerFormModal = observer(function SchedulerFormModal(props: Props) {
  const { isOpen, onClose, onSubmit, scheduler } = props;
  const isEdit = !!scheduler;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<SchedulerFormValues>({ defaultValues: emptyValues });

  useEffect(() => {
    if (!isOpen) return;
    if (scheduler) {
      reset({
        slug: scheduler.slug,
        name: scheduler.name,
        description: scheduler.description ?? "",
        prompt: scheduler.prompt,
        color: scheduler.color || COLOR_PALETTE[0],
        is_enabled: scheduler.is_enabled,
      });
    } else {
      reset(emptyValues);
    }
  }, [isOpen, scheduler, reset]);

  const handleFormSubmit: SubmitHandler<SchedulerFormValues> = async (values) => {
    await onSubmit(values);
  };

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(handleFormSubmit)} className="flex flex-col gap-5 p-5">
        <div className="text-18 font-medium text-primary">
          {isEdit ? t("Edit scheduler") : t("New scheduler")}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="scheduler-slug" className="text-13 font-medium text-primary">
            {t("Slug")}
          </label>
          <Controller
            control={control}
            name="slug"
            rules={{
              required: t("Slug is required."),
              pattern: {
                value: SLUG_PATTERN,
                message: "Use lowercase letters, numbers, and dashes only.",
              },
            }}
            render={({ field: { value, onChange, ref } }) => (
              <Input
                id="scheduler-slug"
                name="slug"
                type="text"
                value={value}
                onChange={onChange}
                ref={ref}
                disabled={isEdit}
                hasError={Boolean(errors.slug)}
                placeholder={t("security-audit")}
                className="w-full"
              />
            )}
          />
          <p className="text-12 text-secondary">{t("Lowercase identifier used in URLs. Cannot be changed after creation.")}</p>
          {errors.slug?.message && <p className="text-12 text-danger-primary">{errors.slug.message}</p>}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="scheduler-name" className="text-13 font-medium text-primary">
            {t("Name")}
          </label>
          <Controller
            control={control}
            name="name"
            rules={{ required: t("Name is required.") }}
            render={({ field: { value, onChange, ref } }) => (
              <Input
                id="scheduler-name"
                name="name"
                type="text"
                value={value}
                onChange={onChange}
                ref={ref}
                hasError={Boolean(errors.name)}
                placeholder={t("Security audit")}
                className="w-full"
              />
            )}
          />
          {errors.name?.message && <p className="text-12 text-danger-primary">{errors.name.message}</p>}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="scheduler-description" className="text-13 font-medium text-primary">
            {t("Description")}
          </label>
          <Controller
            control={control}
            name="description"
            render={({ field: { value, onChange, ref } }) => (
              <TextArea
                id="scheduler-description"
                name="description"
                value={value}
                onChange={onChange}
                ref={ref}
                placeholder={t("Short summary shown in the install picker.")}
                className="min-h-[60px] w-full"
              />
            )}
          />
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="scheduler-prompt" className="text-13 font-medium text-primary">
            {t("Prompt")}
          </label>
          <Controller
            control={control}
            name="prompt"
            rules={{ required: t("Prompt is required.") }}
            render={({ field: { value, onChange, ref } }) => (
              <TextArea
                id="scheduler-prompt"
                name="prompt"
                value={value}
                onChange={onChange}
                ref={ref}
                hasError={Boolean(errors.prompt)}
                placeholder={t("Look for outstanding security issues in this project…")}
                className="font-mono min-h-[180px] w-full text-13"
              />
            )}
          />
          <p className="text-12 text-secondary">{t("The base prompt the agent runs each tick. Per-project context is appended at install time, so keep this prompt project-agnostic.")}</p>
          {errors.prompt?.message && <p className="text-12 text-danger-primary">{errors.prompt.message}</p>}
        </div>

        <div className="flex flex-col gap-2">
          <span className="text-13 font-medium text-primary">{t("Color")}</span>
          <Controller
            control={control}
            name="color"
            render={({ field: { value, onChange } }) => (
              <div className="flex flex-wrap items-center gap-2">
                {COLOR_PALETTE.map((c) => {
                  const active = c.toLowerCase() === (value || "").toLowerCase();
                  return (
                    <button
                      key={c}
                      type="button"
                      aria-label={`Color ${c}`}
                      aria-pressed={active}
                      onClick={() => onChange(c)}
                      className={`h-6 w-6 rounded-md border ${active ? "ring-offset-surface-1 ring-primary ring-2 ring-offset-1" : "border-subtle"}`}
                      style={{ backgroundColor: c }}
                    />
                  );
                })}
              </div>
            )}
          />
          <p className="text-12 text-secondary">{t("Used to color this scheduler's blocks on the project calendar.")}</p>
        </div>

        <div className="flex items-start justify-between gap-4">
          <div className="flex flex-col">
            <span className="text-13 font-medium text-primary">{t("Enabled")}</span>
            <span className="text-12 text-secondary">{t("Disabled schedulers cannot be installed on new projects, and existing bindings will not fire.")}</span>
          </div>
          <Controller
            control={control}
            name="is_enabled"
            render={({ field: { value, onChange } }) => <ToggleSwitch value={value} onChange={onChange} size="sm" />}
          />
        </div>

        <div className="flex items-center justify-end gap-3 border-t border-subtle pt-4">
          <Button variant="secondary" onClick={onClose} disabled={isSubmitting} type="button">
            {t("Cancel")}
          </Button>
          <Button variant="primary" type="submit" loading={isSubmitting} disabled={isSubmitting}>
            {isEdit
              ? isSubmitting
                ? t("Saving…")
                : t("Save")
              : isSubmitting
                ? t("Creating…")
                : t("Create scheduler")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
