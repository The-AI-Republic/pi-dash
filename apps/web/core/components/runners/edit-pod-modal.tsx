/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import type { SubmitHandler } from "react-hook-form";
import { Controller, useForm } from "react-hook-form";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { PodService } from "@pi-dash/services";
import type { IPod } from "@pi-dash/types";
import { EModalPosition, EModalWidth, Input, ModalCore, ToggleSwitch } from "@pi-dash/ui";

type Props = {
  isOpen: boolean;
  onClose: () => void;
  /** The pod being edited. ``null`` renders nothing (modal closed). */
  pod: IPod | null;
  onUpdated: (pod: IPod) => void;
};

interface FormValues {
  name: string;
  description: string;
  isDefault: boolean;
}

const podService = new PodService();

/** Strip the ``{PROJECT}_`` prefix so the user edits only the suffix, the
 * same shape the create modal collects. The server re-prefixes on save. */
function suffixOf(pod: IPod): string {
  const prefix = `${pod.project_identifier}_`;
  return pod.name.startsWith(prefix) ? pod.name.slice(prefix.length) : pod.name;
}

export const EditPodModal = observer(function EditPodModal(props: Props) {
  const { isOpen, onClose, pod, onUpdated } = props;
  const { t } = useTranslation();

  const {
    control,
    handleSubmit,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<FormValues>({
    defaultValues: { name: "", description: "", isDefault: false },
  });

  // Re-seed the form each time a pod is opened so stale values from a prior
  // edit never leak into the next one.
  useEffect(() => {
    if (!isOpen || !pod) return;
    reset({
      name: suffixOf(pod),
      description: pod.description ?? "",
      isDefault: pod.is_default,
    });
  }, [isOpen, pod, reset]);

  const onSubmit: SubmitHandler<FormValues> = async (values) => {
    if (!pod) return;
    // Send only what actually changed; an unchanged is_default toggle stays
    // out of the payload so we never re-issue a promote/demote no-op.
    const payload: { name?: string; description?: string; is_default?: boolean } = {};
    const nextName = values.name.trim();
    if (nextName !== suffixOf(pod)) payload.name = nextName;
    if (values.description.trim() !== (pod.description ?? "").trim()) {
      payload.description = values.description.trim();
    }
    if (values.isDefault !== pod.is_default) payload.is_default = values.isDefault;

    if (Object.keys(payload).length === 0) {
      onClose();
      return;
    }

    try {
      const updated = await podService.update(pod.id, payload);
      onUpdated(updated);
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error!"),
        message: err?.error ?? t("Could not update the pod."),
      });
    }
  };

  return (
    <ModalCore isOpen={isOpen && !!pod} handleClose={onClose} position={EModalPosition.CENTER} width={EModalWidth.XXL}>
      <form onSubmit={handleSubmit(onSubmit)} className="flex flex-col gap-5 p-5">
        <div>
          <div className="text-18 font-medium text-primary">{t("Edit pod")}</div>
          <p className="mt-1 text-13 text-secondary">
            {t("Rename the pod, update its description, or make it the project's default.")}
          </p>
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="edit-pod-name" className="text-13 font-medium text-primary">
            {t("Name")}
          </label>
          <Controller
            control={control}
            name="name"
            rules={{
              validate: (v) => v.trim().length > 0 || t("Name is required."),
            }}
            render={({ field }) => <Input {...field} id="edit-pod-name" placeholder={t("beefy")} />}
          />
          <p className="text-12 text-secondary">
            {t("Letters, digits, dashes, and underscores. The project prefix is added automatically.")}
          </p>
          {errors.name && <span className="text-12 text-danger-primary">{errors.name.message}</span>}
        </div>

        <div className="flex flex-col gap-1">
          <label htmlFor="edit-pod-description" className="text-13 font-medium text-primary">
            {t("Description (optional)")}
          </label>
          <Controller
            control={control}
            name="description"
            render={({ field }) => (
              <Input {...field} id="edit-pod-description" placeholder={t("Where this pod runs, what it's for, etc.")} />
            )}
          />
        </div>

        <Controller
          control={control}
          name="isDefault"
          render={({ field }) => {
            // A pod that is already the default cannot be un-defaulted here —
            // the project must always have a default, and the server only
            // transfers the flag by promoting another pod. Offer the toggle
            // only to promote a non-default pod.
            const alreadyDefault = pod?.is_default ?? false;
            return (
              <div className="flex items-start justify-between gap-3">
                <div>
                  <div className="text-13 font-medium text-primary">{t("Project default")}</div>
                  <p className="text-12 text-secondary">
                    {alreadyDefault
                      ? t("This is the project's default pod. Promote another pod to change the default.")
                      : t("New issues without an explicit pod delegate to the project's default.")}
                  </p>
                </div>
                <ToggleSwitch
                  value={field.value}
                  onChange={field.onChange}
                  disabled={alreadyDefault}
                  label={t("Project default")}
                />
              </div>
            );
          }}
        />

        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose} disabled={isSubmitting}>
            {t("Cancel")}
          </Button>
          <Button type="submit" loading={isSubmitting} disabled={isSubmitting}>
            {isSubmitting ? t("Saving…") : t("Save changes")}
          </Button>
        </div>
      </form>
    </ModalCore>
  );
});
