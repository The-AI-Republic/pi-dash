/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo } from "react";
import { observer } from "mobx-react";
import { rrulestr } from "rrule";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EModalPosition, EModalWidth, ModalCore, TextArea, ToggleSwitch } from "@pi-dash/ui";

interface EditFormValues {
  dtstart: string; // YYYY-MM-DDTHH:mm (local wall time)
  tzid: string;
  rrule: string;
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

// "YYYY-MM-DDTHH:mm" parsed as local time → UTC ISO for the API.
const localToIsoUTC = (local: string): string => {
  if (!local) return "";
  const d = new Date(local);
  if (Number.isNaN(d.getTime())) return "";
  return d.toISOString();
};

const pad2 = (n: number) => n.toString().padStart(2, "0");

// UTC ISO from the binding → "YYYY-MM-DDTHH:mm" in the user's local tz, so
// datetime-local can render it. Browser's Date handles the offset.
const isoUTCToLocalInput = (iso: string): string => {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return (
    `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}` +
    `T${pad2(d.getHours())}:${pad2(d.getMinutes())}`
  );
};

export const EditSchedulerBindingModal = observer(function EditSchedulerBindingModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, binding, onUpdated } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<EditFormValues>({
    defaultValues: { dtstart: "", tzid: "UTC", rrule: "", extra_context: "", enabled: true },
  });

  useEffect(() => {
    if (!isOpen || !binding) return;
    reset({
      dtstart: isoUTCToLocalInput(binding.dtstart),
      tzid: binding.tzid || "UTC",
      rrule: binding.rrule || "",
      extra_context: binding.extra_context ?? "",
      enabled: binding.enabled,
    });
  }, [isOpen, binding, reset]);

  const rruleStr = watch("rrule");
  const dtstartLocal = watch("dtstart");
  const humanRule = useMemo(() => {
    if (!rruleStr) return t("scheduler_bindings.install_modal.rrule_empty_help");
    try {
      const anchor = new Date(dtstartLocal || Date.now());
      const cleaned = rruleStr.replace(/^RRULE:/i, "");
      return rrulestr(cleaned, { dtstart: anchor }).toText();
    } catch {
      return t("scheduler_bindings.install_modal.rrule_invalid_help");
    }
  }, [rruleStr, dtstartLocal, t]);

  const handleFormSubmit: SubmitHandler<EditFormValues> = async (values) => {
    if (!binding) return;
    try {
      const updated = await schedulerService.updateBinding(workspaceSlug, projectId, binding.id, {
        dtstart: localToIsoUTC(values.dtstart),
        tzid: values.tzid.trim() || "UTC",
        rrule: values.rrule.trim(),
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
      const err = e as { error?: string; rrule?: string[]; dtstart?: string[]; tzid?: string[] } | null;
      const detail =
        err?.error ??
        err?.rrule?.[0] ??
        err?.dtstart?.[0] ??
        err?.tzid?.[0] ??
        t("scheduler_bindings.toast.update_failed");
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

        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          <div className="flex flex-col gap-1">
            <label htmlFor="edit-binding-dtstart" className="text-13 font-medium text-primary">
              {t("scheduler_bindings.install_modal.dtstart_label")}
            </label>
            <Controller
              control={control}
              name="dtstart"
              rules={{ required: t("scheduler_bindings.install_modal.errors.dtstart_required") }}
              render={({ field }) => (
                <input
                  {...field}
                  type="datetime-local"
                  id="edit-binding-dtstart"
                  className="bg-layer-0 focus:ring-accent-primary rounded-md border border-subtle px-3 py-2 text-13 text-primary focus:ring-1 focus:outline-none"
                />
              )}
            />
            {errors.dtstart && <span className="text-red-500 text-12">{errors.dtstart.message}</span>}
          </div>
          <div className="flex flex-col gap-1">
            <label htmlFor="edit-binding-tzid" className="text-13 font-medium text-primary">
              {t("scheduler_bindings.install_modal.tzid_label")}
            </label>
            <Controller
              control={control}
              name="tzid"
              render={({ field }) => (
                <input
                  {...field}
                  type="text"
                  id="edit-binding-tzid"
                  placeholder="UTC"
                  className="bg-layer-0 focus:ring-accent-primary rounded-md border border-subtle px-3 py-2 text-13 text-primary focus:ring-1 focus:outline-none"
                />
              )}
            />
          </div>
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="edit-binding-rrule" className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.rrule_label")}
          </label>
          <Controller
            control={control}
            name="rrule"
            render={({ field }) => (
              <TextArea
                {...field}
                id="edit-binding-rrule"
                rows={2}
                placeholder="FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=9;BYMINUTE=0"
                hasError={!!errors.rrule}
              />
            )}
          />
          <p className="text-12 text-secondary">
            {t("scheduler_bindings.install_modal.rrule_help")} — <span className="text-primary">{humanRule}</span>
          </p>
          {errors.rrule && <span className="text-red-500 text-12">{errors.rrule.message}</span>}
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
