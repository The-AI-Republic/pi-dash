/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Controller } from "react-hook-form";
import type { Control, FieldValues, Path } from "react-hook-form";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { PodService } from "@pi-dash/services";
import type { IPod } from "@pi-dash/types";

const podService = new PodService();

type Props<T extends FieldValues> = {
  control: Control<T>;
  /** RHF field holding the pod id, or "" for "use project default". */
  name: Path<T>;
  projectId: string | undefined;
};

/**
 * Pod override selector for the install/edit binding modals. The empty value
 * means "use the project's default pod" — the backend late-binds the actual
 * pod at fire time, so leaving this unset keeps the prior behavior. Generic
 * over the form values type so both modals can reuse it.
 */
export function BindingPodField<T extends FieldValues>({ control, name, projectId }: Props<T>) {
  const { t } = useTranslation();
  // Pods are project-scoped; key the cache by project.
  const { data: pods, isLoading } = useSWR<IPod[]>(
    projectId ? ["scheduler-binding-pods", projectId] : null,
    projectId ? () => podService.list(undefined, projectId) : null
  );

  return (
    <div className="flex flex-col gap-1">
      <label htmlFor="binding-pod" className="text-13 font-medium text-primary">
        {t("Pod")}
      </label>
      <Controller
        control={control}
        name={name}
        render={({ field }) => (
          <select
            {...field}
            id="binding-pod"
            disabled={isLoading}
            className="rounded-md border border-subtle bg-surface-1 px-3 py-2 text-13 text-primary focus:ring-1 focus:ring-accent-strong focus:outline-none"
          >
            <option value="">{t("Project default pod")}</option>
            {(pods ?? []).map((pod) => (
              <option key={pod.id} value={pod.id}>
                {pod.name}
                {pod.is_default ? ` (${t("default")})` : ""}
              </option>
            ))}
          </select>
        )}
      />
      <p className="text-12 text-secondary">
        {t(
          "Which pod's runners serve this scheduler's runs. Leave on the project default unless you need a specific machine fleet."
        )}
      </p>
    </div>
  );
}
