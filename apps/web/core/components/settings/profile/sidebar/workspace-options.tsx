/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { CirclePlus, Mails } from "lucide-react";
import { observer } from "mobx-react";
import { useParams } from "react-router";
// pi dash imports
import { useTranslation } from "@pi-dash/i18n";
import { ChevronRightIcon } from "@pi-dash/propel/icons";
import type { IWorkspace } from "@pi-dash/types";
import { cn } from "@pi-dash/utils";
// components
import { SettingsSidebarItem } from "@/components/settings/sidebar/item";
import {
  SettingsSidebarWorkspaceSettingGroups,
  useAccessibleWorkspaceSettings,
} from "@/components/settings/sidebar/workspace-setting-groups";
import { WorkspaceLogo } from "@/components/workspace/logo";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";

type TWorkspaceRowProps = {
  workspace: IWorkspace;
  defaultExpanded: boolean;
};

/**
 * One workspace in the settings navigator. Expands in place to show that workspace's
 * settings pages; each link is a normal href to `/:workspaceSlug/settings/...`, so
 * selecting an item of a workspace other than the current one navigates into that
 * workspace and switches context the same way any cross-workspace link does.
 *
 * A workspace where the user may open nothing renders as a plain row - no chevron and
 * no expansion - rather than an empty group.
 */
const WorkspaceSettingsRow = observer(function WorkspaceSettingsRow(props: TWorkspaceRowProps) {
  const { workspace, defaultExpanded } = props;
  // derived values
  const groups = useAccessibleWorkspaceSettings(workspace.slug);
  const hasAccessibleSettings = groups.length > 0;
  // states
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);

  const logo = <WorkspaceLogo logo={workspace.logo_url} name={workspace.name} classNames="shrink-0" />;

  if (!hasAccessibleSettings) {
    return (
      <SettingsSidebarItem
        as="link"
        href={`/${workspace.slug}/`}
        iconNode={logo}
        label={workspace.name}
        isActive={false}
      />
    );
  }

  return (
    <div className="shrink-0">
      <button
        type="button"
        onClick={() => setIsExpanded((prev) => !prev)}
        aria-expanded={isExpanded}
        className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-body-sm-medium text-secondary transition-colors hover:bg-layer-transparent-hover"
      >
        {logo}
        <span className="truncate">{workspace.name}</span>
        <ChevronRightIcon
          className={cn("ml-auto size-3.5 shrink-0 text-tertiary transition-transform", { "rotate-90": isExpanded })}
        />
      </button>
      {isExpanded && (
        <SettingsSidebarWorkspaceSettingGroups
          workspaceSlug={workspace.slug}
          className="mt-0.5 flex flex-col gap-y-1 pl-4"
        />
      )}
    </div>
  );
});

export const ProfileSettingsSidebarWorkspaceOptions = observer(function ProfileSettingsSidebarWorkspaceOptions() {
  // params
  const { workspaceSlug } = useParams();
  // store hooks
  const { workspaces } = useWorkspace();
  // translation
  const { t } = useTranslation();

  return (
    <div className="shrink-0">
      <div className="p-2 text-caption-md-medium text-tertiary capitalize">{t("Workspace")}</div>
      <div className="flex flex-col">
        {Object.values(workspaces).map((workspace) => (
          <WorkspaceSettingsRow
            key={workspace.id}
            workspace={workspace}
            // The workspace you are already in starts expanded; the rest start collapsed so
            // a user in many workspaces is not handed a wall of links.
            defaultExpanded={workspace.slug === workspaceSlug}
          />
        ))}
        <div className="mt-1.5">
          <SettingsSidebarItem
            as="link"
            href="/create-workspace/"
            icon={CirclePlus}
            label={t("Create workspace")}
            isActive={false}
          />
          <SettingsSidebarItem
            as="link"
            href="/invitations/"
            icon={Mails}
            label={t("Workspace invites")}
            isActive={false}
          />
        </div>
      </div>
    </div>
  );
});
