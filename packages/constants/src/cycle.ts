/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// types
export const CYCLE_STATUS: {
  i18n_label: string;
  value: "current" | "upcoming" | "completed" | "draft";
  i18n_title: string;
  color: string;
  textColor: string;
  bgColor: string;
}[] = [
  {
    i18n_label: "Days left",
    value: "current",
    i18n_title: "In progress",
    color: "#F59E0B",
    textColor: "text-amber-500",
    bgColor: "bg-amber-50",
  },
  {
    i18n_label: "Yet to start",
    value: "upcoming",
    i18n_title: "Yet to start",
    color: "#3F76FF",
    textColor: "text-blue-500",
    bgColor: "bg-indigo-50",
  },
  {
    i18n_label: "Completed",
    value: "completed",
    i18n_title: "Completed",
    color: "#16A34A",
    textColor: "text-success-primary",
    bgColor: "bg-success-subtle",
  },
  {
    i18n_label: "Draft",
    value: "draft",
    i18n_title: "Draft",
    color: "#525252",
    textColor: "text-tertiary",
    bgColor: "bg-surface-2",
  },
];
