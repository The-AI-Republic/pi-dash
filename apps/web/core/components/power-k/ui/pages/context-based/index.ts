/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export * from "./root";

// components
import type { TPowerKContextType } from "@/components/power-k/core/types";
// pi dash web imports
import { CONTEXT_ENTITY_MAP_EXTENDED } from "@/pi-dash-web/components/command-palette/power-k/pages/context-based";

export type TContextEntityMap = {
  i18n_title: string;
  i18n_indicator: string;
};

export const CONTEXT_ENTITY_MAP: Record<TPowerKContextType, TContextEntityMap> = {
  "work-item": {
    i18n_title: "Work item actions",
    i18n_indicator: "Work item",
  },
  page: {
    i18n_title: "Page actions",
    i18n_indicator: "Page",
  },
  cycle: {
    i18n_title: "Cycle actions",
    i18n_indicator: "Cycle",
  },
  module: {
    i18n_title: "Module actions",
    i18n_indicator: "Module",
  },
  ...CONTEXT_ENTITY_MAP_EXTENDED,
};
