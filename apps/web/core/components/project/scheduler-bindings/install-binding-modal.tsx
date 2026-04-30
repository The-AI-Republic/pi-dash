/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IScheduler, ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EModalPosition, EModalWidth, Input, ModalCore, TextArea, ToggleSwitch } from "@pi-dash/ui";

interface InstallFormValues {
  scheduler: string;
  cron: string;
  extra_context: string;
  enabled: boolean;
}

type Props = {
  isOpen: boolean;
  onClose: () => void;
  workspaceSlug: string;
  projectId: string;
  /**
   * The full set of workspace schedulers. The picker filters to enabled
   * ones not already bound to this project so the user can't double-install.
   */
  availableSchedulers: IScheduler[];
  /** Bindings that already exist on this project, used to filter the picker. */
  existingBindings: ISchedulerBinding[];
  onInstalled: (binding: ISchedulerBinding) => void;
};

const DEFAULT_VALUES: InstallFormValues = {
  scheduler: "",
  // 09:00 UTC daily — non-controversial default; the help text spells out
  // the format and timezone so users editing it know what they're picking.
  cron: "0 9 * * *",
  extra_context: "",
  enabled: true,
};

const schedulerService = new SchedulerService();

export const InstallSchedulerBindingModal = observer(function InstallSchedulerBindingModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, availableSchedulers, existingBindings, onInstalled } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<InstallFormValues>({ defaultValues: DEFAULT_VALUES });

  // Filter to enabled workspace schedulers that don't already have a
  // binding on this project. The cloud also enforces this with a unique
  // constraint, but filtering client-side keeps the picker clean and
  // means the install button is only enabled when there's something
  // valid to install.
  const installable = useMemo(() => {
    const boundIds = new Set(existingBindings.map((b) => b.scheduler));
    return availableSchedulers.filter((s) => s.is_enabled && !boundIds.has(s.id));
  }, [availableSchedulers, existingBindings]);

  useEffect(() => {
    if (!isOpen) return;
    // Pre-select the first installable scheduler so submitting without
    // touching the picker still produces a valid payload.
    reset({
      ...DEFAULT_VALUES,
      scheduler: installable[0]?.id ?? "",
    });
  }, [isOpen, installable, reset]);

  const handleFormSubmit: SubmitHandler<InstallFormValues> = async (values) => {
    try {
      const binding = await schedulerService.createBinding(workspaceSlug, projectId, {
        scheduler: values.scheduler,
        // Cloud's serializer.is_valid() requires this even though the
        // projectId is already in the URL — see scheduler.service.ts.
        project: projectId,
        cron: values.cron.trim(),
        extra_context: values.extra_context.trim(),
        enabled: values.enabled,
      });
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("scheduler_bindings.toast.installed_title"),
        message: t("scheduler_bindings.toast.installed_message"),
      });
      onInstalled(binding);
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string; cron?: string[]; scheduler?: string[] } | null;
      const detail =
        err?.error ?? err?.cron?.[0] ?? err?.scheduler?.[0] ?? t("scheduler_bindings.toast.install_failed");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("scheduler_bindings.toast.error_title"),
        message: detail,
      });
    }
  };

  // Empty-state branch: no schedulers to install. Render a helpful body
  // pointing the operator at the workspace catalog rather than an
  // unsubmittable form.
  if (installable.length === 0) {
    return (
      <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XL}>
        <div className="flex flex-col gap-4 p-5">
          <div className="text-18 font-medium text-primary">
            {t("scheduler_bindings.install_modal.none_available_title")}
          </div>
          <p className="text-13 text-secondary">{t("scheduler_bindings.install_modal.none_available_body")}</p>
          <div className="flex justify-end">
            <Button variant="secondary" onClick={onClose}>
              {t("scheduler_bindings.install_modal.cancel")}
            </Button>
          </div>
        </div>
      </ModalCore>
    );
  }

  return (
    <ModalCore isOpen={isOpen} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(handleFormSubmit)} className="flex flex-col gap-5 p-5">
        <div className="text-18 font-medium text-primary">{t("scheduler_bindings.install_modal.title")}</div>

        <div className="flex flex-col gap-1">
          <label htmlFor="binding-scheduler" className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.scheduler_label")}
          </label>
          <Controller
            control={control}
            name="scheduler"
            rules={{ required: t("scheduler_bindings.install_modal.errors.scheduler_required") }}
            render={({ field }) => (
              <select
                {...field}
                id="binding-scheduler"
                className="bg-layer-0 focus:ring-accent-primary rounded-md border border-subtle px-3 py-2 text-13 text-primary focus:ring-1 focus:outline-none"
              >
                {installable.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} ({s.slug})
                  </option>
                ))}
              </select>
            )}
          />
          <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.scheduler_help")}</p>
          {errors.scheduler && <span className="text-red-500 text-12">{errors.scheduler.message}</span>}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="binding-cron" className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.cron_label")}
          </label>
          <Controller
            control={control}
            name="cron"
            rules={{ required: t("scheduler_bindings.install_modal.errors.cron_required") }}
            render={({ field }) => (
              <Input
                {...field}
                id="binding-cron"
                placeholder={t("scheduler_bindings.install_modal.cron_placeholder")}
                hasError={!!errors.cron}
              />
            )}
          />
          <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.cron_help")}</p>
          {errors.cron && <span className="text-red-500 text-12">{errors.cron.message}</span>}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="binding-extra-context" className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.extra_context_label")}
          </label>
          <Controller
            control={control}
            name="extra_context"
            render={({ field }) => (
              <TextArea
                {...field}
                id="binding-extra-context"
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
            {isSubmitting
              ? t("scheduler_bindings.install_modal.installing")
              : t("scheduler_bindings.install_modal.install")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
