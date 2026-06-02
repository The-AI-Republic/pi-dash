/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
// mobx
import { observer } from "mobx-react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { Hotel } from "lucide-react";
// pi dash ui
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useLocalStorage } from "@pi-dash/hooks";
import { useTranslation } from "@pi-dash/i18n";
import { MembersPropertyIcon, CheckIcon, ProjectIcon, CloseIcon } from "@pi-dash/propel/icons";
import { cn, getFileURL } from "@pi-dash/utils";
// hooks
import { useCommandPalette } from "@/hooks/store/use-command-palette";
import { useProject } from "@/hooks/store/use-project";
import { useWorkspace } from "@/hooks/store/use-workspace";
import { useUser, useUserPermissions } from "@/hooks/store/user";
// pi dash web constants

export const NoProjectsEmptyState = observer(function NoProjectsEmptyState() {
  // navigation
  const { workspaceSlug } = useParams();
  // store hooks
  const { allowPermissions } = useUserPermissions();
  const { toggleCreateProjectModal } = useCommandPalette();
  const { data: currentUser } = useUser();
  const { joinedProjectIds } = useProject();
  const { currentWorkspace: activeWorkspace } = useWorkspace();
  // local storage
  const { storedValue, setValue } = useLocalStorage(`quickstart-guide-${workspaceSlug}`, {
    hide: false,
    visited_members: false,
    visited_workspace: false,
    visited_profile: false,
  });
  const { t } = useTranslation();
  // derived values
  const canCreateProject = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
    EUserPermissionsLevel.WORKSPACE
  );
  const isWorkspaceAdmin = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE);

  const EMPTY_STATE_DATA = [
    {
      id: "create-project",
      title: "Create a project",
      description: "Most things start with a project in Pi Dash.",
      icon: <ProjectIcon className="size-4" />,
      flag: "projects",
      cta: {
        text: "Get started",
        onClick: (e: React.MouseEvent<HTMLButtonElement, MouseEvent>) => {
          if (!canCreateProject) return;
          e.preventDefault();
          e.stopPropagation();
          toggleCreateProjectModal(true);
        },
        disabled: !canCreateProject,
      },
    },
    {
      id: "invite-team",
      title: "Invite your team",
      description: "Build, ship, and manage with coworkers.",
      icon: <MembersPropertyIcon className="size-4" />,
      flag: "visited_members",
      cta: {
        text: "Get them in",
        link: `/${workspaceSlug}/settings/members`,
        disabled: !isWorkspaceAdmin,
      },
    },
    {
      id: "configure-workspace",
      title: "Set up your workspace.",
      description: "Turn features on or off or go beyond that.",
      icon: <Hotel className="size-4" />,
      flag: "visited_workspace",
      cta: {
        text: "Configure this workspace",
        link: "settings",
        disabled: !isWorkspaceAdmin,
      },
    },
    {
      id: "personalize-account",
      title: "Make Pi Dash yours.",
      description: "Choose your picture, colors, and more.",
      icon:
        currentUser?.avatar_url && currentUser?.avatar_url.trim() !== "" ? (
          <Link href={`/${workspaceSlug}/profile/${currentUser?.id}`}>
            <span className="relative flex size-4 items-center justify-center rounded-full p-4 text-on-color capitalize">
              <img
                src={getFileURL(currentUser?.avatar_url)}
                className="absolute top-0 left-0 h-full w-full rounded-full object-cover"
                alt={currentUser?.display_name || currentUser?.email}
              />
            </span>
          </Link>
        ) : (
          <Link href={`/${workspaceSlug}/profile/${currentUser?.id}`}>
            <span className="relative flex size-4 items-center justify-center rounded-full bg-[#028375] p-4 text-13 text-on-color capitalize">
              {(currentUser?.email ?? currentUser?.display_name ?? "?")[0]}
            </span>
          </Link>
        ),
      flag: "visited_profile",
      cta: {
        text: "Personalize now",
        link: `/settings/profile/general`,
        disabled: false,
      },
    },
  ];
  const isComplete = (type: string) => {
    switch (type) {
      case "projects":
        return joinedProjectIds?.length > 0;
      case "visited_members":
        return (activeWorkspace?.total_members || 0) >= 2;
      case "visited_workspace":
        return storedValue?.visited_workspace;
      case "visited_profile":
        return storedValue?.visited_profile;
    }
  };

  if (storedValue?.hide || (joinedProjectIds?.length > 0 && (activeWorkspace?.total_members || 0) >= 2)) return null;

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <div className="text-14 font-semibold text-tertiary">{t("Your quickstart guide")}</div>
        <button
          className="flex items-center gap-1 text-13 font-medium text-tertiary"
          onClick={() => {
            if (!storedValue) return;
            setValue({ ...storedValue, hide: true });
          }}
        >
          <CloseIcon className="size-4" />
          {t("Not right now")}
        </button>
      </div>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {EMPTY_STATE_DATA.map((item) => {
          const isStateComplete = isComplete(item.flag);
          return (
            <div key={item.id} className="flex flex-col rounded-xl border border-subtle bg-layer-2 p-4">
              <div
                className={cn("mb-3 grid size-9 place-items-center rounded-full bg-surface-2 text-placeholder", {
                  "bg-accent-primary/10 text-accent-primary": !isStateComplete,
                })}
              >
                <span className="my-auto text-24">{item.icon}</span>
              </div>
              <h3 className="mb-2 text-13 font-medium text-primary">{t(item.title)}</h3>
              <p className="mb-2 text-11 text-tertiary">{t(item.description)}</p>
              {isStateComplete ? (
                <div className="flex w-fit items-center gap-2 rounded-full bg-[#17a34a] p-1">
                  <CheckIcon className="size-3 text-accent-primary text-on-color" />
                </div>
              ) : (
                !item.cta.disabled &&
                (item.cta.link ? (
                  <Link
                    href={item.cta.link}
                    onClick={(e) => {
                      if (!storedValue) {
                        e.stopPropagation();
                        e.preventDefault();
                        return;
                      }
                      setValue({
                        ...storedValue,
                        [item.flag]: true,
                      });
                    }}
                    className={cn("text-13 font-medium text-accent-primary hover:text-accent-secondary", {})}
                  >
                    {t(item.cta.text)}
                  </Link>
                ) : (
                  <button
                    type="button"
                    className="text-left text-13 font-medium text-accent-primary hover:text-accent-secondary"
                    onClick={item.cta.onClick}
                  >
                    {t(item.cta.text)}
                  </button>
                ))
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
});
