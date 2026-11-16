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
import { deriveSchedulerSlug } from "./definition-helpers";

type RhfPath<T extends FieldValues> = Path<T>;

type Props<T extends FieldValues> = {
  control: Control<T>;
  errors: FieldErrors<T>;
  nameName: RhfPath<T>;
  descriptionName: RhfPath<T>;
  promptName: RhfPath<T>;
  colorName: RhfPath<T>;
  /**
   * Return false to skip required/pattern validation, e.g. when these fields
   * sit behind an inactive tab. RHF keeps hidden fields registered by
   * default, so gating happens inside the validate closures (read at submit
   * time) rather than by swapping the rules objects.
   */
  isActive?: () => boolean;
};

/**
 * Scheduler definition fields (name, description, prompt, color) shared
 * between the workspace Schedulers form modal and the project-side
 * "New Scheduler" modal's create path.
 *
 * There is no slug input: the server derives the slug from the name, and a
 * name that collides with another scheduler comes back as a `name` error.
 *
 * Generic over the form values type so each consumer keeps its own value
 * shape, mirroring {@link BindingScheduleFields}.
 */
export function SchedulerDefinitionFields<T extends FieldValues>({
  control,
  errors,
  nameName,
  descriptionName,
  promptName,
  colorName,
  isActive,
}: Props<T>) {
  const { t } = useTranslation();

  const active = () => (isActive ? isActive() : true);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const nameErr = (errors as any)[nameName as string];
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
            validate: (v) => {
              if (!active()) return true;
              const name = String(v ?? "").trim();
              if (!name) return t("Name is required.");
              if (!deriveSchedulerSlug(name)) return t("Name must contain at least one letter or digit.");
              return true;
            },
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
