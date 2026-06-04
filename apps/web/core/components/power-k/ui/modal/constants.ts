/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash web imports
import { POWER_K_MODAL_PAGE_DETAILS_EXTENDED } from "@/pi-dash-web/components/command-palette/power-k/constants";
// local imports
import type { TPowerKPageType } from "../../core/types";

export type TPowerKModalPageDetails = {
  i18n_placeholder: string;
};

export const POWER_K_MODAL_PAGE_DETAILS: Record<TPowerKPageType, TPowerKModalPageDetails> = {
  "open-workspace": {
    i18n_placeholder: "Open a workspace",
  },
  "open-project": {
    i18n_placeholder: "Open a project",
  },
  "open-workspace-setting": {
    i18n_placeholder: "Open a workspace setting",
  },
  "open-project-cycle": {
    i18n_placeholder: "Open a cycle",
  },
  "open-project-module": {
    i18n_placeholder: "Open a module",
  },
  "open-project-view": {
    i18n_placeholder: "Open a project view",
  },
  "open-project-setting": {
    i18n_placeholder: "Open a project setting",
  },
  "update-work-item-state": {
    i18n_placeholder: "Change state",
  },
  "update-work-item-priority": {
    i18n_placeholder: "Change priority",
  },
  "update-work-item-assignee": {
    i18n_placeholder: "Assign to",
  },
  "update-work-item-estimate": {
    i18n_placeholder: "Change estimate",
  },
  "update-work-item-cycle": {
    i18n_placeholder: "Add to cycle",
  },
  "update-work-item-module": {
    i18n_placeholder: "Add to modules",
  },
  "update-work-item-labels": {
    i18n_placeholder: "Add labels",
  },
  "update-module-member": {
    i18n_placeholder: "Change members",
  },
  "update-module-status": {
    i18n_placeholder: "Change status",
  },
  "update-theme": {
    i18n_placeholder: "Change theme",
  },
  "update-timezone": {
    i18n_placeholder: "Change timezone",
  },
  "update-start-of-week": {
    i18n_placeholder: "Change first day of week",
  },
  "update-language": {
    i18n_placeholder: "Change language",
  },
  ...POWER_K_MODAL_PAGE_DETAILS_EXTENDED,
};
