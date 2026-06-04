/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash imports
import type { TProjectAppliedDisplayFilterKeys, TProjectOrderByOptions } from "@pi-dash/types";
// local imports

export type TNetworkChoiceIconKey = "Lock" | "Globe2";

export type TNetworkChoice = {
  key: 0 | 2;
  labelKey: string;
  i18n_label: string;
  description: string;
  iconKey: TNetworkChoiceIconKey;
};

export const NETWORK_CHOICES: TNetworkChoice[] = [
  {
    key: 0,
    labelKey: "Private",
    i18n_label: "Private",
    description: "Accessible only by invite",
    iconKey: "Lock",
  },
  {
    key: 2,
    labelKey: "Public",
    i18n_label: "Public",
    description: "Anyone in the workspace except Guests can join",
    iconKey: "Globe2",
  },
];

export const GROUP_CHOICES = {
  backlog: {
    key: "backlog",
    i18n_label: "Backlog",
  },
  unstarted: {
    key: "unstarted",
    i18n_label: "Unstarted",
  },
  started: {
    key: "started",
    i18n_label: "Started",
  },
  review: {
    key: "review",
    i18n_label: "Review",
  },
  completed: {
    key: "completed",
    i18n_label: "Completed",
  },
  cancelled: {
    key: "cancelled",
    i18n_label: "Cancelled",
  },
};

export const PROJECT_AUTOMATION_MONTHS = [
  { i18n_label: "{months, plural, one{# month} other{# months}}", value: 1 },
  { i18n_label: "{months, plural, one{# month} other{# months}}", value: 3 },
  { i18n_label: "{months, plural, one{# month} other{# months}}", value: 6 },
  { i18n_label: "{months, plural, one{# month} other{# months}}", value: 9 },
  { i18n_label: "{months, plural, one{# month} other{# months}}", value: 12 },
];

export const PROJECT_ORDER_BY_OPTIONS: {
  key: TProjectOrderByOptions;
  i18n_label: string;
}[] = [
  {
    key: "sort_order",
    i18n_label: "Manual",
  },
  {
    key: "name",
    i18n_label: "Name",
  },
  {
    key: "created_at",
    i18n_label: "Created date",
  },
  {
    key: "members_length",
    i18n_label: "Number of members",
  },
];

export const PROJECT_DISPLAY_FILTER_OPTIONS: {
  key: TProjectAppliedDisplayFilterKeys;
  i18n_label: string;
}[] = [
  {
    key: "my_projects",
    i18n_label: "My projects",
  },
  {
    key: "archived_projects",
    i18n_label: "Archived",
  },
];

export const PROJECT_ERROR_MESSAGES = {
  permissionError: {
    i18n_title: "You don't have permission to perform this action.",
    i18n_message: undefined,
  },
  cycleDeleteError: {
    i18n_title: "Error",
    i18n_message: "Failed to delete cycle",
  },
  moduleDeleteError: {
    i18n_title: "Error",
    i18n_message: "Failed to delete module",
  },
  issueDeleteError: {
    i18n_title: "Error",
    i18n_message: "Failed to delete work item",
  },
};

export enum EProjectFeatureKey {
  WORK_ITEMS = "work_items",
  CYCLES = "cycles",
  MODULES = "modules",
  VIEWS = "views",
  PAGES = "pages",
  INTAKE = "intake",
}
