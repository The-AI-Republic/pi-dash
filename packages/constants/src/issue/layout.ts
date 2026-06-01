/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { EIssueLayoutTypes } from "@pi-dash/types";

export type TIssueLayout = "list" | "kanban" | "calendar" | "spreadsheet" | "gantt";

export type TIssueLayoutMap = Record<
  EIssueLayoutTypes,
  {
    key: EIssueLayoutTypes;
    i18n_title: string;
    i18n_label: string;
  }
>;

export const SITES_ISSUE_LAYOUTS: {
  key: TIssueLayout;
  titleTranslationKey: string;
  icon: string;
}[] = [
  {
    key: "list",
    icon: "List",
    titleTranslationKey: "List",
  },
  {
    key: "kanban",
    icon: "Kanban",
    titleTranslationKey: "Board",
  },
];

export const ISSUE_LAYOUT_MAP: TIssueLayoutMap = {
  [EIssueLayoutTypes.LIST]: {
    key: EIssueLayoutTypes.LIST,
    i18n_title: "List Layout",
    i18n_label: "List",
  },
  [EIssueLayoutTypes.KANBAN]: {
    key: EIssueLayoutTypes.KANBAN,
    i18n_title: "Board Layout",
    i18n_label: "Board",
  },
  [EIssueLayoutTypes.CALENDAR]: {
    key: EIssueLayoutTypes.CALENDAR,
    i18n_title: "Calendar Layout",
    i18n_label: "Calendar",
  },
  [EIssueLayoutTypes.SPREADSHEET]: {
    key: EIssueLayoutTypes.SPREADSHEET,
    i18n_title: "Table Layout",
    i18n_label: "Table",
  },
  [EIssueLayoutTypes.GANTT]: {
    key: EIssueLayoutTypes.GANTT,
    i18n_title: "Timeline Layout",
    i18n_label: "Timeline",
  },
};

export const ISSUE_LAYOUTS: {
  key: EIssueLayoutTypes;
  i18n_title: string;
  i18n_label: string;
}[] = Object.values(ISSUE_LAYOUT_MAP);
