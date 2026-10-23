// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Navbar theme switch (parity/specs/space.md SPC-011). An icon-only
// control showing the theme a click will move to: moon while light,
// sun while dark. The accessible name and the tooltip name the same
// move, and the gate driver finds the control by its test id.
import { IconButton, Tooltip } from "@pidash/kit";
import { Moon, Sun } from "lucide-react";
import * as React from "react";

import { spaceMessage } from "../lib/spaceMessages.js";
import { useSpaceTheme } from "../lib/spaceTheme.js";

export function SpaceThemeSwitch(): React.ReactElement {
  const { theme, toggle } = useSpaceTheme();
  const label = spaceMessage(theme === "light" ? "theme.switchToDark" : "theme.switchToLight");
  return (
    <Tooltip content={label}>
      <IconButton label={label} data-testid="space-theme-switch" onClick={toggle}>
        {theme === "light" ? (
          <Moon aria-hidden="true" size={14} className="shrink-0" />
        ) : (
          <Sun aria-hidden="true" size={14} className="shrink-0" />
        )}
      </IconButton>
    </Tooltip>
  );
}
