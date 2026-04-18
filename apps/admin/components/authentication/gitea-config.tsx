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

export const GiteaConfiguration = observer(function GiteaConfiguration(props: Props) {
  const { disabled, updateConfig } = props;
  // store
  const { formattedConfig } = useInstance();
  // derived values
  const GiteaConfig = formattedConfig?.IS_GITEA_ENABLED ?? "";
  const GiteaConfigured =
    !!formattedConfig?.GITEA_HOST && !!formattedConfig?.GITEA_CLIENT_ID && !!formattedConfig?.GITEA_CLIENT_SECRET;

  return (
    <>
      {GiteaConfigured ? (
        <div className="flex items-center gap-4">
          <Link href="/authentication/gitea" className={cn(getButtonStyling("link", "base"), "font-medium")}>
            Edit
          </Link>
          <ToggleSwitch
            value={Boolean(parseInt(GiteaConfig))}
            onChange={() => {
              Boolean(parseInt(GiteaConfig)) === true
                ? updateConfig("IS_GITEA_ENABLED", "0")
                : updateConfig("IS_GITEA_ENABLED", "1");
            }}
            size="sm"
            disabled={disabled}
          />
        </div>
      ) : (
        <Link href="/authentication/gitea" className={cn(getButtonStyling("secondary", "base"), "text-tertiary")}>
          <Settings2 className="h-4 w-4 p-0.5 text-tertiary" />
          Configure
        </Link>
      )}
    </>
  );
});
