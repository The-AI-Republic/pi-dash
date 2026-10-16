/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Controller } from "react-hook-form";
import type { Control, FieldErrors, FieldValues, Path } from "react-hook-form";
import { useTranslation } from "@pi-dash/i18n";
import { Input, TextArea } from "@pi-dash/ui";
import { SCHEDULER_COLOR_PALETTE as COLOR_PALETTE } from "@/components/project/scheduler-bindings/constants";

const SLUG_PATTERN = /^[a-z0-9][a-z0-9-]*$/;

type RhfPath<T extends FieldValues> = Path<T>;

type Props<T extends FieldValues> = {
  control: Control<T>;
  errors: FieldErrors<T>;
  nameName: RhfPath<T>;
  slugName: RhfPath<T>;
  descriptionName: RhfPath<T>;
  promptName: RhfPath<T>;
  colorName: RhfPath<T>;
  /** Lock the slug field (edit mode — slug is immutable after creation). */
  slugDisabled?: boolean;
  /**
   * Return false to skip required/pattern validation, e.g. when these fields
   * sit behind an inactive tab. RHF keeps hidden fields registered by
   * default, so gating happens inside the validate closures (read at submit
   * time) rather than by swapping the rules objects.
   */
  isActive?: () => boolean;
};

/**
 * Scheduler definition fields (name, slug, description, prompt, color) shared
 * between the workspace Schedulers form modal and the project-side
 * "New Scheduler" modal's create path.
 *
 * Generic over the form values type so each consumer keeps its own value
 * shape, mirroring {@link BindingScheduleFields}.
 */
export function SchedulerDefinitionFields<T extends FieldValues>({
  control,
  errors,
  nameName,
  slugName,
  descriptionName,
  promptName,
  colorName,
  slugDisabled,
  isActive,
}: Props<T>) {
  const { t } = useTranslation();

  const active = () => (isActive ? isActive() : true);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const nameErr = (errors as any)[nameName as string];
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const slugErr = (errors as any)[slugName as string];
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const promptErr = (errors as any)[promptName as string];

  return (
    <>
      <div className="flex flex-col gap-1">
        <label htmlFor="scheduler-name" className="text-13 font-medium text-primary">
          {t("Name")}
        </label>
        <Controller
          control={control}
          name={nameName}
          rules={{
            validate: (v) => !active() || String(v ?? "").trim().length > 0 || t("Name is required."),
          }}
          render={({ field: { value, onChange, ref } }) => (
            <Input
              id="scheduler-name"
              name="name"
              type="text"
              value={value}
              onChange={onChange}
              ref={ref}
              hasError={Boolean(nameErr)}
              placeholder={t("Security audit")}
              className="w-full"
            />
          )}
        />
        {nameErr?.message && <p className="text-12 text-danger-primary">{String(nameErr.message)}</p>}
      </div>

      <div className="flex flex-col gap-1">
        <label htmlFor="scheduler-slug" className="text-13 font-medium text-primary">
          {t("Slug")}
        </label>
        <Controller
          control={control}
          name={slugName}
          rules={{
            validate: (v) => {
              if (!active()) return true;
              const s = String(v ?? "");
              if (!s) return t("Slug is required.");
              if (!SLUG_PATTERN.test(s)) return "Use lowercase letters, numbers, and dashes only.";
              return true;
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
              disabled={slugDisabled}
              hasError={Boolean(slugErr)}
              placeholder={t("security-audit")}
              className="w-full"
            />
          )}
        />
        <p className="text-12 text-secondary">
          {t("Lowercase identifier used in URLs. Cannot be changed after creation.")}
        </p>
        {slugErr?.message && <p className="text-12 text-danger-primary">{String(slugErr.message)}</p>}
      </div>

      <div className="flex flex-col gap-1">
        <label htmlFor="scheduler-description" className="text-13 font-medium text-primary">
          {t("Description")}
        </label>
        <Controller
          control={control}
          name={descriptionName}
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
          name={promptName}
          rules={{
            validate: (v) => !active() || String(v ?? "").trim().length > 0 || t("Prompt is required."),
          }}
          render={({ field: { value, onChange, ref } }) => (
            <TextArea
              id="scheduler-prompt"
              name="prompt"
              value={value}
              onChange={onChange}
              ref={ref}
              hasError={Boolean(promptErr)}
              placeholder={t("Look for outstanding security issues in this project…")}
              className="font-mono min-h-[180px] w-full text-13"
            />
          )}
        />
        <p className="text-12 text-secondary">
          {t(
            "The base prompt the agent runs each tick. Per-project context is appended at install time, so keep this prompt project-agnostic."
          )}
        </p>
        {promptErr?.message && <p className="text-12 text-danger-primary">{String(promptErr.message)}</p>}
      </div>

      <div className="flex flex-col gap-2">
        <span className="text-13 font-medium text-primary">{t("Color")}</span>
        <Controller
          control={control}
          name={colorName}
          render={({ field: { value, onChange } }) => (
            <div className="flex flex-wrap items-center gap-2">
              {COLOR_PALETTE.map((c) => {
                const selected = c.toLowerCase() === String(value ?? "").toLowerCase();
                return (
                  <button
                    key={c}
                    type="button"
                    aria-label={`Color ${c}`}
                    aria-pressed={selected}
                    onClick={() => onChange(c)}
                    className={`h-6 w-6 rounded-md border ${selected ? "ring-offset-surface-1 ring-primary ring-2 ring-offset-1" : "border-subtle"}`}
                    style={{ backgroundColor: c }}
                  />
                );
              })}
            </div>
          )}
        />
        <p className="text-12 text-secondary">{t("Used to color this scheduler's blocks on the project calendar.")}</p>
      </div>
    </>
  );
}
