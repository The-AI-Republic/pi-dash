/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import Link from "next/link";
// icons
import { Settings2 } from "lucide-react";
// apple pi dash internal packages
import { getButtonStyling } from "@apple-pi-dash/propel/button";
import type { TInstanceAuthenticationMethodKeys } from "@apple-pi-dash/types";
import { ToggleSwitch } from "@apple-pi-dash/ui";
import { cn } from "@apple-pi-dash/utils";
// hooks
import { useInstance } from "@/hooks/store";

type Props = {
  disabled: boolean;
  updateConfig: (key: TInstanceAuthenticationMethodKeys, value: string) => void;
};

export const GitlabConfiguration = observer(function GitlabConfiguration(props: Props) {
  const { disabled, updateConfig } = props;
  // store
  const { formattedConfig } = useInstance();
  // derived values
  const enableGitlabConfig = formattedConfig?.IS_GITLAB_ENABLED ?? "";
  const isGitlabConfigured = !!formattedConfig?.GITLAB_CLIENT_ID && !!formattedConfig?.GITLAB_CLIENT_SECRET;

  return (
    <>
      {isGitlabConfigured ? (
        <div className="flex items-center gap-4">
          <Link href="/authentication/gitlab" className={cn(getButtonStyling("link", "base"), "font-medium")}>
            Edit
          </Link>
          <ToggleSwitch
            value={Boolean(parseInt(enableGitlabConfig))}
            onChange={() => {
              const newEnableGitlabConfig = Boolean(parseInt(enableGitlabConfig)) === true ? "0" : "1";
              updateConfig("IS_GITLAB_ENABLED", newEnableGitlabConfig);
            }}
            size="sm"
            disabled={disabled}
          />
        </div>
      ) : (
        <Link href="/authentication/gitlab" className={cn(getButtonStyling("secondary", "base"), "text-tertiary")}>
          <Settings2 className="h-4 w-4 p-0.5 text-tertiary" />
          Configure
        </Link>
      )}
    </>
  );
});
