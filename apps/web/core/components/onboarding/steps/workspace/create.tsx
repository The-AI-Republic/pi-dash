/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { Controller, useForm } from "react-hook-form";
import { CircleCheck } from "lucide-react";
// pi dash imports
import { ORGANIZATION_SIZE, RESTRICTED_URLS } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type { IUser, IWorkspace } from "@pi-dash/types";
import { Spinner } from "@pi-dash/ui";
import { cn, validateWorkspaceName, validateSlug } from "@pi-dash/utils";
// hooks
import { useInstance } from "@/hooks/store/use-instance";
import { useWorkspace } from "@/hooks/store/use-workspace";
import { useUserProfile, useUserSettings } from "@/hooks/store/user";
// services
import { WorkspaceService } from "@/services/workspace.service";
// local components
import { CommonOnboardingHeader } from "../common";

type Props = {
  user: IUser | undefined;
  onComplete: (skipInvites?: boolean) => void;
  handleCurrentViewChange: () => void;
  hasInvitations?: boolean;
};

const workspaceService = new WorkspaceService();

export const WorkspaceCreateStep = observer(function WorkspaceCreateStep({
  user,
  onComplete,
  handleCurrentViewChange,
  hasInvitations = false,
}: Props) {
  // states
  const [slugError, setSlugError] = useState(false);
  const [invalidSlug, setInvalidSlug] = useState(false);
  // pi dash hooks
  const { t } = useTranslation();
  // store hooks
  const { config } = useInstance();
  const { updateUserProfile } = useUserProfile();
  const { fetchCurrentUserSettings } = useUserSettings();
  const { createWorkspace, fetchWorkspaces } = useWorkspace();

  const isWorkspaceCreationDisabled = config?.is_workspace_creation_disabled ?? false;

  // form info
  const {
    handleSubmit,
    control,
    setValue,
    formState: { errors, isSubmitting, isValid },
  } = useForm<IWorkspace>({
    defaultValues: {
      name: "",
      slug: "",
      organization_size: "",
    },
    mode: "onChange",
  });

  const handleCreateWorkspace = async (formData: IWorkspace) => {
    if (isSubmitting) return;

    try {
      const res = (await workspaceService.workspaceSlugCheck(formData.slug)) as { status: boolean };
      if (res.status === true && !RESTRICTED_URLS.includes(formData.slug)) {
        setSlugError(false);
        try {
          const workspaceResponse = await createWorkspace(formData);
          setToast({
            type: TOAST_TYPE.SUCCESS,
            title: t("Success"),
            message: t("Workspace created successfully"),
          });
          await fetchWorkspaces();
          await completeStep(workspaceResponse.id);
          onComplete(formData.organization_size === "Just myself");
        } catch {
          setToast({
            type: TOAST_TYPE.ERROR,
            title: t("Error"),
            message: t("Workspace could not be created. Please try again."),
          });
        }
      } else {
        setSlugError(true);
      }
    } catch {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: t("Error"),
        message: t("Workspace could not be created. Please try again."),
      });
    }
  };

  const completeStep = async (workspaceId: string) => {
    if (!user) return;
    await updateUserProfile({
      last_workspace_id: workspaceId,
    });
    await fetchCurrentUserSettings();
  };

  const isButtonDisabled = !isValid || invalidSlug || isSubmitting;

  if (isWorkspaceCreationDisabled) {
    return (
      <div className="flex flex-col gap-10">
        <span className="text-center text-14 text-tertiary">
          You don&apos;t seem to have any invites to a workspace and your instance admin has restricted creation of new
          workspaces. Please ask a workspace owner or admin to invite you to a workspace first and come back to this
          screen to join.
        </span>
      </div>
    );
  }
  return (
    <form
      className="flex flex-col gap-10"
      onSubmit={(e) => {
        void handleSubmit(handleCreateWorkspace)(e);
      }}
    >
      <CommonOnboardingHeader title="Create your workspace" description="All your work — unified." />
      <div className="flex flex-col gap-8">
        <div className="flex flex-col gap-2">
          <label
            className="text-13 font-medium text-tertiary after:ml-0.5 after:text-danger-primary after:content-['*']"
            htmlFor="name"
          >
            {t("Name your workspace")}
          </label>
          <Controller
            control={control}
            name="name"
            rules={{
              required: t("This field is required"),
              validate: (value) => validateWorkspaceName(value, true),
              maxLength: {
                value: 80,
                message: t("Limit your name to 80 characters."),
              },
            }}
            render={({ field: { value, ref, onChange } }) => (
              <div className="relative flex items-center rounded-md">
                <input
                  id="name"
                  name="name"
                  type="text"
                  value={value}
                  onChange={(event) => {
                    onChange(event.target.value);
                    setValue("name", event.target.value);
                    setValue("slug", event.target.value.toLocaleLowerCase().trim().replace(/ /g, "-"), {
                      shouldValidate: true,
                    });
                  }}
                  placeholder="Enter workspace name"
                  ref={ref}
                  className={cn(
                    "w-full rounded-md border border-strong bg-surface-1 px-3 py-2 text-secondary transition-all duration-200 placeholder:text-placeholder focus:border-transparent focus:ring-2 focus:ring-accent-strong focus:outline-none",
                    {
                      "border-strong": !errors.name,
                      "border-danger-strong": errors.name,
                    }
                  )}
                  // eslint-disable-next-line jsx-a11y/no-autofocus
                  autoFocus
                />
              </div>
            )}
          />
          {errors.name && <span className="text-13 text-danger-primary">{errors.name.message}</span>}
        </div>
        <div className="flex flex-col gap-2">
          <label
            className="text-13 font-medium text-tertiary after:ml-0.5 after:text-danger-primary after:content-['*']"
            htmlFor="slug"
          >
            {t("Set your workspace's URL")}
          </label>
          <Controller
            control={control}
            name="slug"
            rules={{
              required: t("This field is required"),
              maxLength: {
                value: 48,
                message: t("Limit your URL to 48 characters."),
              },
            }}
            render={({ field: { value, ref, onChange } }) => (
              <div
                className={cn(
                  "flex w-full items-center rounded-md border border-strong bg-surface-1 px-3 py-2 text-secondary transition-all duration-200 focus:border-transparent focus:ring-2 focus:ring-accent-strong focus:outline-none",
                  {
                    "border-strong": !errors.name,
                    "border-danger-strong": errors.name,
                  }
                )}
              >
                <span className={cn("rounded-md pr-0 whitespace-nowrap text-secondary")}>
                  {window && window.location.host}/
                </span>
                <input
                  id="slug"
                  name="slug"
                  type="text"
                  value={value.toLocaleLowerCase().trim().replace(/ /g, "-")}
                  onChange={(e) => {
                    const validation = validateSlug(e.target.value);
                    if (validation === true) setInvalidSlug(false);
                    else setInvalidSlug(true);
                    onChange(e.target.value.toLowerCase());
                  }}
                  ref={ref}
                  placeholder={t("Type or paste a URL")}
                  className={cn(
                    "ring-none w-full rounded-md border-none bg-surface-1 px-3 py-0 pl-0 text-secondary outline-none placeholder:text-placeholder"
                  )}
                />
              </div>
            )}
          />
          <p className="text-13 text-tertiary">{t("You can only edit the slug of the URL")}</p>
          {slugError && (
            <p className="-mt-3 text-13 text-danger-primary">
              {t("Workspace URL is already taken!")}
            </p>
          )}
          {invalidSlug && (
            <p className="text-13 text-danger-primary">{t("URLs can contain only ('-') and alphanumeric characters.")}</p>
          )}
          {errors.slug && <span className="text-13 text-danger-primary">{errors.slug.message}</span>}
        </div>
        <div className="flex flex-col gap-2">
          <label
            className="text-13 font-medium text-tertiary after:ml-0.5 after:text-danger-primary after:content-['*']"
            htmlFor="organization_size"
          >
            {t("How many people will use this workspace?")}
          </label>
          <div className="w-full">
            <Controller
              name="organization_size"
              control={control}
              rules={{ required: t("This field is required") }}
              render={({ field: { value, onChange } }) => (
                <div className="flex flex-wrap gap-3">
                  {ORGANIZATION_SIZE.map((size) => {
                    const isSelected = value === size;
                    return (
                      <button
                        key={size}
                        onClick={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          onChange(size);
                        }}
                        className={`flex items-center justify-between gap-1 rounded-lg border px-3 py-2 text-13 transition-all duration-200 ${
                          isSelected
                            ? "border-subtle bg-layer-1 text-secondary"
                            : "border-subtle text-tertiary hover:border-strong"
                        }`}
                      >
                        <CircleCheck className={cn("size-4 text-placeholder", isSelected && "text-secondary")} />

                        <span className="font-medium">{size}</span>
                      </button>
                    );
                  })}
                </div>
              )}
            />
            {errors.organization_size && (
              <span className="text-13 text-danger-primary">{errors.organization_size.message}</span>
            )}
          </div>
        </div>
      </div>
      <div className="flex flex-col gap-4">
        <Button variant="primary" type="submit" size="xl" className="w-full" disabled={isButtonDisabled}>
          {isSubmitting ? <Spinner height="20px" width="20px" /> : t("Create workspace")}
        </Button>
        {hasInvitations && (
          <Button variant="ghost" size="xl" className="w-full" onClick={handleCurrentViewChange}>
            Join existing workspace
          </Button>
        )}
      </div>
    </form>
  );
});
