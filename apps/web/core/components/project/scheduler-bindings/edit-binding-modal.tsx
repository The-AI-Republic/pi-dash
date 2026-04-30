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
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EModalPosition, EModalWidth, Input, ModalCore, TextArea, ToggleSwitch } from "@pi-dash/ui";

interface EditFormValues {
  cron: string;
  extra_context: string;
  enabled: boolean;
}

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  projectId: string;
  binding: ISchedulerBinding | null;
  onUpdated: (binding: ISchedulerBinding) => void;
};

const schedulerService = new SchedulerService();

export const EditSchedulerBindingModal = observer(function EditSchedulerBindingModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, binding, onUpdated } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<EditFormValues>({
    defaultValues: { cron: "", extra_context: "", enabled: true },
  });

  useEffect(() => {
    if (!isOpen || !binding) return;
    reset({
      cron: binding.cron,
      extra_context: binding.extra_context ?? "",
      enabled: binding.enabled,
    });
  }, [isOpen, binding, reset]);

  const handleFormSubmit: SubmitHandler<EditFormValues> = async (values) => {
    if (!binding) return;
    try {
      const updated = await schedulerService.updateBinding(workspaceSlug, projectId, binding.id, {
        cron: values.cron.trim(),
        extra_context: values.extra_context.trim(),
        enabled: values.enabled,
      });
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("scheduler_bindings.toast.updated_title"),
        message: t("scheduler_bindings.toast.updated_message"),
      });
      onUpdated(updated);
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string; cron?: string[] } | null;
      const detail = err?.error ?? err?.cron?.[0] ?? t("scheduler_bindings.toast.update_failed");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("scheduler_bindings.toast.error_title"),
        message: detail,
      });
    }
  };

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(handleFormSubmit)} className="flex flex-col gap-5 p-5">
        <div className="text-18 font-medium text-primary">
          {t("scheduler_bindings.edit_modal.title")}
          {binding && <span className="ml-2 text-13 text-secondary">— {binding.scheduler_name}</span>}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="edit-binding-cron" className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.cron_label")}
          </label>
          <Controller
            control={control}
            name="cron"
            rules={{ required: t("scheduler_bindings.install_modal.errors.cron_required") }}
            render={({ field }) => (
              <Input
                {...field}
                id="edit-binding-cron"
                placeholder={t("scheduler_bindings.install_modal.cron_placeholder")}
                hasError={!!errors.cron}
              />
            )}
          />
          <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.cron_help")}</p>
          {errors.cron && <span className="text-red-500 text-12">{errors.cron.message}</span>}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="edit-binding-extra-context" className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.extra_context_label")}
          </label>
          <Controller
            control={control}
            name="extra_context"
            render={({ field }) => (
              <TextArea
                {...field}
                id="edit-binding-extra-context"
                rows={4}
                placeholder={t("scheduler_bindings.install_modal.extra_context_placeholder")}
              />
            )}
          />
          <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.extra_context_help")}</p>
        </div>

        <Controller
          control={control}
          name="enabled"
          render={({ field }) => (
            <div className="flex items-center justify-between gap-4">
              <div className="flex flex-col">
                <span className="text-13 font-medium text-primary">
                  {t("scheduler_bindings.install_modal.enabled_label")}
                </span>
                <span className="text-12 text-secondary">{t("scheduler_bindings.install_modal.enabled_help")}</span>
              </div>
              <ToggleSwitch value={field.value} onChange={field.onChange} />
            </div>
          )}
        />

        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose} disabled={isSubmitting}>
            {t("scheduler_bindings.install_modal.cancel")}
          </Button>
          <Button type="submit" loading={isSubmitting} disabled={isSubmitting}>
            {isSubmitting ? t("scheduler_bindings.edit_modal.saving") : t("scheduler_bindings.edit_modal.save")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
