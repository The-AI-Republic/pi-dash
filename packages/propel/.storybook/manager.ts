/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { addons } from "storybook/manager-api";
import { create } from "storybook/theming";

const piDashTheme = create({
  base: "dark",
  brandTitle: "Pi Dash UI",
  brandUrl: "https://pi-dash.so",
  brandImage: "pi-dash-lockup-light.svg",
  brandTarget: "_self",
});

addons.setConfig({
  theme: piDashTheme,
});
