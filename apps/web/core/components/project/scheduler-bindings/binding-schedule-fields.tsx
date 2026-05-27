/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useState } from "react";
import { Controller } from "react-hook-form";
import type { Control, FieldErrors, FieldValues, Path } from "react-hook-form";
import { useTranslation } from "@pi-dash/i18n";
import { TextArea, ToggleSwitch } from "@pi-dash/ui";
import { humanizeRrule } from "./rrule-text";

type RhfPath<T extends FieldValues> = Path<T>;

type Props<T extends FieldValues> = {
  control: Control<T>;
  errors: FieldErrors<T>;
  dtstartName: RhfPath<T>;
  tzidName: RhfPath<T>;
  rruleName: RhfPath<T>;
  extraContextName: RhfPath<T>;
  enabledName: RhfPath<T>;
  /** Current values used to render the live RRULE humanizer hint. */
  watchDtstart: string;
  watchRrule: string;
};

/**
 * Shared form fields for the install and edit binding modals: dtstart
 * picker, tzid select, RRULE textarea + live `toText()` preview,
 * extra-context textarea, enabled toggle.
 *
 * Generic over the form values type so install and edit can keep their
 * own per-form value shapes.
 */
export function BindingScheduleFields<T extends FieldValues>({
  control,
  errors,
  dtstartName,
  tzidName,
  rruleName,
  extraContextName,
  enabledName,
  watchDtstart,
  watchRrule,
}: Props<T>) {
  const { t } = useTranslation();

  const humanRule = useMemo(() => {
    if (!watchRrule) return t("scheduler_bindings.install_modal.rrule_empty_help");
    const anchor = watchDtstart ? new Date(watchDtstart) : new Date();
    return humanizeRrule(watchRrule, anchor) ?? t("scheduler_bindings.install_modal.rrule_invalid_help");
  }, [watchRrule, watchDtstart, t]);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const dtstartErr = (errors as any)[dtstartName as string];
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const rruleErr = (errors as any)[rruleName as string];

  return (
    <>
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        <div className="flex flex-col gap-1">
          <label htmlFor={`field-${String(dtstartName)}`} className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.dtstart_label")}
          </label>
          <Controller
            control={control}
            name={dtstartName}
            rules={{ required: t("scheduler_bindings.install_modal.errors.dtstart_required") }}
            render={({ field }) => (
              <input
                {...field}
                type="datetime-local"
                id={`field-${String(dtstartName)}`}
                className="bg-layer-0 focus:ring-accent-primary rounded-md border border-subtle px-3 py-2 text-13 text-primary focus:ring-1 focus:outline-none"
              />
            )}
          />
          <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.dtstart_help")}</p>
          {dtstartErr && <span className="text-red-500 text-12">{String(dtstartErr.message ?? "")}</span>}
        </div>
        <div className="flex flex-col gap-1">
          <label htmlFor={`field-${String(tzidName)}`} className="text-13 font-medium text-primary">
            {t("scheduler_bindings.install_modal.tzid_label")}
          </label>
          <Controller
            control={control}
            name={tzidName}
            render={({ field }) => <TzidSelect id={`field-${String(tzidName)}`} {...field} />}
          />
          <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.tzid_help")}</p>
        </div>
      </div>

      <div className="flex flex-col gap-1">
        <label htmlFor={`field-${String(rruleName)}`} className="text-13 font-medium text-primary">
          {t("scheduler_bindings.install_modal.rrule_label")}
        </label>
        <Controller
          control={control}
          name={rruleName}
          render={({ field }) => (
            <TextArea
              {...field}
              id={`field-${String(rruleName)}`}
              rows={2}
              placeholder="FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=9;BYMINUTE=0"
              hasError={!!rruleErr}
            />
          )}
        />
        <p className="text-12 text-secondary">
          {t("scheduler_bindings.install_modal.rrule_help")} — <span className="text-primary">{humanRule}</span>
        </p>
        {rruleErr && <span className="text-red-500 text-12">{String(rruleErr.message ?? "")}</span>}
      </div>

      <div className="flex flex-col gap-1">
        <label htmlFor={`field-${String(extraContextName)}`} className="text-13 font-medium text-primary">
          {t("scheduler_bindings.install_modal.extra_context_label")}
        </label>
        <Controller
          control={control}
          name={extraContextName}
          render={({ field }) => (
            <TextArea
              {...field}
              id={`field-${String(extraContextName)}`}
              rows={4}
              placeholder={t("scheduler_bindings.install_modal.extra_context_placeholder")}
            />
          )}
        />
        <p className="text-12 text-secondary">{t("scheduler_bindings.install_modal.extra_context_help")}</p>
      </div>

      <Controller
        control={control}
        name={enabledName}
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
    </>
  );
}

// ---------------------------------------------------------------------------
// TzidSelect — IANA timezone dropdown.
// ---------------------------------------------------------------------------

type TzidSelectProps = {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  name?: string;
};

function TzidSelect({ id, value, onChange, onBlur, name }: TzidSelectProps) {
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
      id={id}
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
