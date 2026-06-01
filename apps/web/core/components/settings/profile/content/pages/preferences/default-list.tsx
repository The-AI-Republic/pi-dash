/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// components
import { ThemeSwitcher } from "@/pi-dash-web/components/preferences/theme-switcher";

export const ProfileSettingsDefaultPreferencesList = observer(function ProfileSettingsDefaultPreferencesList() {
  return (
    <div className="flex flex-col gap-y-1">
      <ThemeSwitcher
        option={{
          id: "theme",
          i18n_title: "Theme",
          i18n_description: "Select or customize your interface color scheme.",
        }}
      />
    </div>
  );
});
