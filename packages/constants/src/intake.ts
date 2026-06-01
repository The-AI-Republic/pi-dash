/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TInboxIssueStatus } from "@pi-dash/types";
import { EInboxIssueStatus } from "@pi-dash/types";

export const INBOX_STATUS: {
  key: string;
  status: TInboxIssueStatus;
  i18n_title: string;
  i18n_description: () => string;
}[] = [
  {
    key: "pending",
    i18n_title: "Pending",
    status: EInboxIssueStatus.PENDING,
    i18n_description: () => "Pending",
  },
  {
    key: "declined",
    i18n_title: "Declined",
    status: EInboxIssueStatus.DECLINED,
    i18n_description: () => "Declined",
  },
  {
    key: "snoozed",
    i18n_title: "Snoozed",
    status: EInboxIssueStatus.SNOOZED,
    i18n_description: () => "{days, plural, one{# day} other{# days}} to go",
  },
  {
    key: "accepted",
    i18n_title: "Accepted",
    status: EInboxIssueStatus.ACCEPTED,
    i18n_description: () => "Accepted",
  },
  {
    key: "duplicate",
    i18n_title: "Duplicate",
    status: EInboxIssueStatus.DUPLICATE,
    i18n_description: () => "Duplicate",
  },
];

export const INBOX_ISSUE_ORDER_BY_OPTIONS = [
  {
    key: "issue__created_at",
    i18n_label: "Created at",
  },
  {
    key: "issue__updated_at",
    i18n_label: "Updated at",
  },
  {
    key: "issue__sequence_id",
    i18n_label: "ID",
  },
];

export const INBOX_ISSUE_SORT_BY_OPTIONS = [
  {
    key: "asc",
    i18n_label: "Ascending",
  },
  {
    key: "desc",
    i18n_label: "Descending",
  },
];

export enum EPastDurationFilters {
  TODAY = "today",
  YESTERDAY = "yesterday",
  LAST_7_DAYS = "last_7_days",
  LAST_30_DAYS = "last_30_days",
}

export const PAST_DURATION_FILTER_OPTIONS: {
  name: string;
  value: string;
}[] = [
  {
    name: "Today",
    value: EPastDurationFilters.TODAY,
  },
  {
    name: "Yesterday",
    value: EPastDurationFilters.YESTERDAY,
  },
  {
    name: "Last 7 days",
    value: EPastDurationFilters.LAST_7_DAYS,
  },
  {
    name: "Last 30 days",
    value: EPastDurationFilters.LAST_30_DAYS,
  },
];
