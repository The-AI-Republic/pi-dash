/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { observer } from "mobx-react";
import { rrulestr } from "rrule";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IScheduler, ISchedulerBinding } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EModalPosition, EModalWidth, ModalCore, TextArea, ToggleSwitch } from "@pi-dash/ui";

interface InstallFormValues {
  scheduler: string;
  dtstart: string; // ISO local datetime: "YYYY-MM-DDTHH:mm"
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
  /**
   * The full set of workspace schedulers. The picker filters to enabled
   * ones not already bound to this project so the user can't double-install.
   */
  availableSchedulers: IScheduler[];
  /** Bindings that already exist on this project, used to filter the picker. */
  existingBindings: ISchedulerBinding[];
  onInstalled: (binding: ISchedulerBinding) => void;
};

const pad2 = (n: number) => n.toString().padStart(2, "0");

// Defaults match the legacy "0 9 * * *" cron (every morning at 9): a
// FREQ=DAILY RRULE anchored at the next 9am in the user's local browser tz.
// dtstart is a "local-input" string the user can adjust before submitting;
// the submit handler converts it to UTC ISO for the API.
const defaultDtstartLocal = (): string => {
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(9, 0, 0, 0);
  // datetime-local needs "YYYY-MM-DDTHH:mm" — no seconds, no tz.
  return (
    `${tomorrow.getFullYear()}-${pad2(tomorrow.getMonth() + 1)}-${pad2(tomorrow.getDate())}` +
    `T${pad2(tomorrow.getHours())}:${pad2(tomorrow.getMinutes())}`
  );
};

const DEFAULT_VALUES = (): InstallFormValues => ({
  scheduler: "",
  dtstart: defaultDtstartLocal(),
  tzid: Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",
  rrule: "FREQ=DAILY",
  extra_context: "",
  enabled: true,
});

const schedulerService = new SchedulerService();

// Convert "YYYY-MM-DDTHH:mm" (local wall time) → UTC ISO string for the API.
// The user types times in their browser's local tz; the binding's tzid is a
// separate field that drives wall-clock semantics on the backend (today
// informational; future PRs honor it during expansion).
const localToIsoUTC = (local: string): string => {
  if (!local) return "";
  // Browser parses "YYYY-MM-DDTHH:mm" as local time.
  const d = new Date(local);
  if (Number.isNaN(d.getTime())) return "";
  return d.toISOString();
};

export const InstallSchedulerBindingModal = observer(function InstallSchedulerBindingModal(props: Props) {
  const { isOpen, onClose, workspaceSlug, projectId, availableSchedulers, existingBindings, onInstalled } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<InstallFormValues>({ defaultValues: DEFAULT_VALUES() });

  // Filter to enabled workspace schedulers that don't already have a
  // binding on this project. The cloud also enforces this with a unique
  // constraint, but filtering client-side keeps the picker clean.
  const installable = useMemo(() => {
    const boundIds = new Set(existingBindings.map((b) => b.scheduler));
    return availableSchedulers.filter((s) => s.is_enabled && !boundIds.has(s.id));
  }, [availableSchedulers, existingBindings]);

  // Same closed→open guard as the legacy cron form: SWR may revalidate the
  // workspace schedulers query while the modal is open; we don't want to
  // reset the form mid-edit.
  const wasOpen = useRef(false);
  useEffect(() => {
    if (isOpen && !wasOpen.current) {
      reset({
        ...DEFAULT_VALUES(),
        scheduler: installable[0]?.id ?? "",
      });
    }
    wasOpen.current = isOpen;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, reset]);

  // Humanize the current RRULE string for the help text under the input.
  // `rrule.toText()` produces phrases like "every day" / "every weekday at 9".
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

  const handleFormSubmit: SubmitHandler<InstallFormValues> = async (values) => {
    try {
      const binding = await schedulerService.createBinding(workspaceSlug, projectId, {
        scheduler: values.scheduler,
        project: projectId,
        dtstart: localToIsoUTC(values.dtstart),
        tzid: values.tzid.trim() || "UTC",
        rrule: values.rrule.trim(),
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
      const err = e as {
        error?: string;
        rrule?: string[];
        dtstart?: string[];
        tzid?: string[];
        scheduler?: string[];
      } | null;
      const detail =
        err?.error ??
        err?.rrule?.[0] ??
        err?.dtstart?.[0] ??
        err?.tzid?.[0] ??
        err?.scheduler?.[0] ??
        t("scheduler_bindings.toast.install_failed");
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("scheduler_bindings.toast.error_title"),
        message: detail,
      });
    }
  };

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

        {/* Series anchor + timezone */}
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          <div className="flex flex-col gap-1">
            <label htmlFor="binding-dtstart" className="text-13 font-medium text-primary">
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
                  id="binding-dtstart"
                  className="bg-layer-0 focus:ring-accent-primary rounded-md border border-subtle px-3 py-2 text-13 text-primary focus:ring-1 focus:outline-none"
                />
              )}
            />
            <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.dtstart_help")}</p>
            {errors.dtstart && <span className="text-red-500 text-12">{errors.dtstart.message}</span>}
          </div>
          <div className="flex flex-col gap-1">
            <label htmlFor="binding-tzid" className="text-13 font-medium text-primary">
              {t("scheduler_bindings.install_modal.tzid_label")}
            </label>
            <Controller
              control={control}
              name="tzid"
              rules={{ required: t("scheduler_bindings.install_modal.errors.tzid_required") }}
              render={({ field }) => <TzidSelect {...field} />}
            />
            <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.tzid_help")}</p>
          </div>
        </div>

        {/* RRULE */}
        <div className="flex flex-col gap-1">
          <label htmlFor="binding-rrule" className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.rrule_label")}
          </label>
          <Controller
            control={control}
            name="rrule"
            render={({ field }) => (
              <TextArea
                {...field}
                id="binding-rrule"
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

// ---------------------------------------------------------------------------
// TzidSelect — small IANA timezone picker.
// ---------------------------------------------------------------------------

type TzidSelectProps = {
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  name?: string;
};

/**
 * Renders a native <select> with the user's resolved timezones. Uses
 * Intl.supportedValuesOf("timeZone") when available; falls back to a curated
 * list of common zones in older runtimes.
 */
function TzidSelect({ value, onChange, onBlur, name }: TzidSelectProps) {
  const [zones] = useState<string[]>(() => {
    type SupportedValuesOfFn = (key: "timeZone") => string[];
    const intlWithSupported = Intl as typeof Intl & { supportedValuesOf?: SupportedValuesOfFn };
    if (typeof intlWithSupported.supportedValuesOf === "function") {
      try {
        return intlWithSupported.supportedValuesOf("timeZone");
      } catch {
        // Fall through to the curated fallback list.
      }
    }
    return ["UTC", "America/Los_Angeles", "America/New_York", "Europe/London", "Europe/Berlin", "Asia/Tokyo"];
  });

  return (
    <select
      name={name}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      onBlur={onBlur}
      className="bg-layer-0 focus:ring-accent-primary rounded-md border border-subtle px-3 py-2 text-13 text-primary focus:ring-1 focus:outline-none"
    >
      {zones.map((z) => (
        <option key={z} value={z}>
          {z}
        </option>
      ))}
    </select>
  );
}
