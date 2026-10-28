/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// --- Instance-admin runner list — support view of per-runner provisioning /
// runner_version / codex_version (.ai_design/managed_runner/design.md §15.1). ---

export interface IInstanceRunnerRow {
  id: string;
  name: string;
  workspace_slug: string | null;
  owner_email: string | null;
  host_label: string;
  provisioning: string;
  status: string;
  runner_version: string;
  codex_version: string | null;
  last_heartbeat_at: string | null;
  created_at: string | null;
  revoked_at: string | null;
}

export interface IInstanceRunnersPage {
  page: number;
  total: number;
  results: IInstanceRunnerRow[];
}
