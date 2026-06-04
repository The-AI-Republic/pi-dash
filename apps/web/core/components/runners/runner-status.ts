/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TRunnerStatus } from "@pi-dash/types";
import type { TBadgeVariant } from "@pi-dash/ui";

export const STATUS_BADGE_VARIANT: Record<TRunnerStatus, TBadgeVariant> = {
  online: "accent-success",
  busy: "accent-primary",
  offline: "accent-neutral",
  revoked: "accent-warning",
};

/** Status -> i18n source string. Kept as a static map (rather than `t(status)`)
 * so the translation extractor can find every key statically. */
export const RUNNER_STATUS_I18N_LABELS: Record<TRunnerStatus, string> = {
  online: "online",
  busy: "busy",
  offline: "offline",
  revoked: "revoked",
};
