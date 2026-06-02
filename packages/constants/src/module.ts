/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// types
import type { TModuleLayoutOptions, TModuleOrderByOptions, TModuleStatus } from "@pi-dash/types";

export const MODULE_STATUS_COLORS: {
  [key in TModuleStatus]: string;
} = {
  backlog: "#a3a3a2",
  planned: "#3f76ff",
  paused: "#525252",
  completed: "#16a34a",
  cancelled: "#ef4444",
  "in-progress": "#f39e1f",
};

export const MODULE_STATUS: {
  i18n_label: string;
  value: TModuleStatus;
  color: string;
  textColor: string;
  bgColor: string;
}[] = [
  {
    i18n_label: "Backlog",
    value: "backlog",
    color: MODULE_STATUS_COLORS.backlog,
    textColor: "text-placeholder",
    bgColor: "bg-layer-1",
  },
  {
    i18n_label: "Planned",
    value: "planned",
    color: MODULE_STATUS_COLORS.planned,
    textColor: "text-blue-500",
    bgColor: "bg-indigo-50",
  },
  {
    i18n_label: "In Progress",
    value: "in-progress",
    color: MODULE_STATUS_COLORS["in-progress"],
    textColor: "text-amber-500",
    bgColor: "bg-amber-50",
  },
  {
    i18n_label: "Paused",
    value: "paused",
    color: MODULE_STATUS_COLORS.paused,
    textColor: "text-tertiary",
    bgColor: "bg-surface-2",
  },
  {
    i18n_label: "Completed",
    value: "completed",
    color: MODULE_STATUS_COLORS.completed,
    textColor: "text-success-primary",
    bgColor: "bg-success-subtle",
  },
  {
    i18n_label: "Cancelled",
    value: "cancelled",
    color: MODULE_STATUS_COLORS.cancelled,
    textColor: "text-danger-primary",
    bgColor: "bg-danger-subtle",
  },
];

export const MODULE_VIEW_LAYOUTS: {
  key: TModuleLayoutOptions;
  i18n_title: string;
}[] = [
  {
    key: "list",
    i18n_title: "List layout",
  },
  {
    key: "board",
    i18n_title: "Gallery layout",
  },
  {
    key: "gantt",
    i18n_title: "Timeline layout",
  },
];

export const MODULE_ORDER_BY_OPTIONS: {
  key: TModuleOrderByOptions;
  i18n_label: string;
}[] = [
  {
    key: "name",
    i18n_label: "Name",
  },
  {
    key: "progress",
    i18n_label: "Progress",
  },
  {
    key: "issues_length",
    i18n_label: "Number of work items",
  },
  {
    key: "target_date",
    i18n_label: "Due date",
  },
  {
    key: "created_at",
    i18n_label: "Created date",
  },
  {
    key: "sort_order",
    i18n_label: "Manual",
  },
];
