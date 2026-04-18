/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { addons } from "storybook/manager-api";
import { create } from "storybook/theming";

const applePiDashTheme = create({
  base: "dark",
  brandTitle: "Apple Pi Dash UI",
  brandUrl: "https://apple-pi-dash.so",
  brandImage: "apple-pi-dash-lockup-light.svg",
  brandTarget: "_self",
});

addons.setConfig({
  theme: applePiDashTheme,
});
