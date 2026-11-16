/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Monitor } from "lucide-react";
import { IconButton } from "@pi-dash/propel/icon-button";
import { Tooltip } from "@pi-dash/propel/tooltip";
import { DesktopEngineSettings, openEngineSettings } from "@/components/desktop-engine-settings";
import { isDesktop } from "@/services/agent-runtime";

/**
 * Corner button beside the sidebar's user menu: "This computer", the Built-in
 * engine settings for this computer's Pi Dash daemon (design §10). The dialog
 * is mounted here, inside the workspace routes, so it knows the workspace.
 */
export function DesktopComputerButton() {
  if (!isDesktop()) return null;
  return (
    <>
      <Tooltip tooltipContent="This computer" position="top">
        <IconButton
          size="lg"
          variant="ghost"
          icon={Monitor}
          onClick={openEngineSettings}
          aria-label="This computer"
          className="shrink-0"
        />
      </Tooltip>
      <DesktopEngineSettings />
    </>
  );
}
