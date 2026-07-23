/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { FC } from "react";
import { useEffect, useState } from "react";
import { observer } from "mobx-react";
import { useParams } from "next/navigation";
import type { UseFormRegister } from "react-hook-form";
import { useForm } from "react-hook-form";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { PlusIcon } from "@pi-dash/propel/icons";
import { setPromiseToast } from "@pi-dash/propel/toast";
import type { IProject, TIssue, EIssueLayoutTypes } from "@pi-dash/types";
import { EIssuesStoreType } from "@pi-dash/types";
import { cn, createIssuePayload } from "@pi-dash/utils";
// components
import { ProjectDropdown } from "@/components/dropdowns/project/dropdown";
// hooks
import { useProject } from "@/hooks/store/use-project";
import { useIssueStoreType } from "@/hooks/use-issue-layout-store";
// pi dash web imports
import { QuickAddIssueFormRoot } from "@/pi-dash-web/components/issues/quick-add";
// local imports
import { CreateIssueToastActionItems } from "../../create-issue-toast-action-items";

export type TQuickAddIssueForm = {
  ref: React.RefObject<HTMLFormElement>;
  isOpen: boolean;
  projectDetail: IProject;
  hasError: boolean;
  register: UseFormRegister<TIssue>;
  onSubmit: () => void;
  isEpic: boolean;
};

export type TQuickAddIssueButton = {
  isEpic?: boolean;
  onClick: () => void;
};

type TQuickAddIssueRoot = {
  isQuickAddOpen?: boolean;
  layout: EIssueLayoutTypes;
  prePopulatedData?: Partial<TIssue>;
  QuickAddButton?: FC<TQuickAddIssueButton>;
  customQuickAddButton?: React.ReactNode;
  containerClassName?: string;
  setIsQuickAddOpen?: (isOpen: boolean) => void;
  quickAddCallback?: (projectId: string | null | undefined, data: TIssue) => Promise<TIssue | undefined>;
  isEpic?: boolean;
};

const defaultValues: Partial<TIssue> = {
  name: "",
};

export const QuickAddIssueRoot = observer(function QuickAddIssueRoot(props: TQuickAddIssueRoot) {
  const {
    isQuickAddOpen,
    layout,
    prePopulatedData,
    QuickAddButton,
    customQuickAddButton,
    containerClassName = "",
    setIsQuickAddOpen,
    quickAddCallback,
    isEpic = false,
  } = props;
  // i18n
  const { t } = useTranslation();
  // router
  const { workspaceSlug, projectId: routerProjectId } = useParams();
  const paramProjectId = routerProjectId?.toString();
  // store hooks
  const storeType = useIssueStoreType();
  const { joinedProjectIds } = useProject();
  // At workspace scope (global "all issues") there is no project in the route,
  // so the user picks one inline before the quick-add creates the issue.
  const isWorkspaceLevel = storeType === EIssuesStoreType.GLOBAL && !paramProjectId;
  // states
  const [isOpen, setIsOpen] = useState(isQuickAddOpen ?? false);
  const [selectedProjectId, setSelectedProjectId] = useState<string | undefined>(undefined);
  // The effective project for creation: the route project, else the picked one.
  const effectiveProjectId = paramProjectId ?? selectedProjectId;

  // Default the inline picker to the first joined project at workspace scope.
  useEffect(() => {
    if (isWorkspaceLevel && !selectedProjectId && joinedProjectIds.length > 0) {
      setSelectedProjectId(joinedProjectIds[0]);
    }
  }, [isWorkspaceLevel, selectedProjectId, joinedProjectIds]);
  // form info
  const {
    reset,
    handleSubmit,
    setFocus,
    register,
    formState: { errors, isSubmitting },
  } = useForm<TIssue>({ defaultValues });

  useEffect(() => {
    if (isQuickAddOpen !== undefined) {
      setIsOpen(isQuickAddOpen);
    }
  }, [isQuickAddOpen]);

  useEffect(() => {
    if (!isOpen) reset({ ...defaultValues });
  }, [isOpen, reset]);

  const handleIsOpen = (isOpen: boolean) => {
    if (isQuickAddOpen !== undefined && setIsQuickAddOpen) {
      setIsQuickAddOpen(isOpen);
    } else {
      setIsOpen(isOpen);
    }
  };

  const onSubmitHandler = async (formData: TIssue) => {
    if (isSubmitting || !workspaceSlug || !effectiveProjectId) return;

    reset({ ...defaultValues });

    const payload = createIssuePayload(effectiveProjectId, {
      ...(prePopulatedData ?? {}),
      ...formData,
    });

    if (quickAddCallback) {
      const quickAddPromise = quickAddCallback(effectiveProjectId, { ...payload });
      setPromiseToast<any>(quickAddPromise, {
        loading: isEpic ? t("Adding epic") : t("Adding work item"),
        success: {
          title: t("Success!"),
          message: () => `${isEpic ? t("Epic created successfully") : t("Work item created successfully")}`,
          actionItems: (data) => (
            // TODO: Translate here
            <CreateIssueToastActionItems
              workspaceSlug={workspaceSlug.toString()}
              projectId={effectiveProjectId}
              issueId={data.id}
              isEpic={isEpic}
            />
          ),
        },
        error: {
          title: t("Error!"),
          message: (err) => err?.message || t("Some error occurred. Please try again."),
        },
      });

      await quickAddPromise;
    }
  };

  if (!effectiveProjectId) return null;

  return (
    <div
      className={cn(
        containerClassName,
        errors && errors?.name && errors?.name?.message ? `border-danger-strong bg-danger-subtle` : ``
      )}
    >
      {isOpen ? (
        <div className="flex w-full items-stretch">
          {isWorkspaceLevel && (
            <div className="flex flex-shrink-0 items-center border-r border-subtle bg-surface-1 px-2">
              <ProjectDropdown
                multiple={false}
                value={effectiveProjectId}
                onChange={(val: string) => setSelectedProjectId(val)}
                buttonVariant="border-with-text"
              />
            </div>
          )}
          <div className="flex-grow">
            <QuickAddIssueFormRoot
              isOpen={isOpen}
              layout={layout}
              prePopulatedData={prePopulatedData}
              projectId={effectiveProjectId}
              hasError={errors && errors?.name && errors?.name?.message ? true : false}
              setFocus={setFocus}
              register={register}
              onSubmit={handleSubmit(onSubmitHandler)}
              onClose={() => handleIsOpen(false)}
              isEpic={isEpic}
            />
          </div>
        </div>
      ) : (
        <>
          {QuickAddButton && <QuickAddButton isEpic={isEpic} onClick={() => handleIsOpen(true)} />}
          {customQuickAddButton && <>{customQuickAddButton}</>}
          {!QuickAddButton && !customQuickAddButton && (
            <button
              className="flex w-full cursor-pointer items-center gap-2 bg-layer-transparent px-2 py-3 hover:bg-layer-transparent-hover"
              onClick={() => handleIsOpen(true)}
            >
              <PlusIcon className="h-3.5 w-3.5 stroke-2" />
              <span className="text-13 font-medium">{t(isEpic ? "New Epic" : "New work item")}</span>
            </button>
          )}
        </>
      )}
    </div>
  );
});
