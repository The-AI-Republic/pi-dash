// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

// "Auto Project Management" (internally: loop). User-facing shapes only — the
// prompt, interval RRULE, and admin metadata are never exposed here.

export interface IAutoPMJob {
  slug: string;
  name: string;
  description: string;
  interval_label: string;
  enabled: boolean;
}

export interface IAutoPMSettings {
  enabled: boolean;
  jobs: IAutoPMJob[];
}

// --- Instance-admin ("Loop") shapes — operator-only, full job fields. ---

export interface ILoopJob {
  id: string;
  slug: string;
  name: string;
  public_name: string;
  public_description: string;
  prompt: string;
  min_role: number;
  enabled: boolean;
  is_builtin: boolean;
  dtstart: string | null;
  rrule: string;
  tzid: string;
  created_at: string | null;
  updated_at: string | null;
  stats?: {
    target_count: number;
    completed: number;
    failed: number;
    skipped: number;
  };
}

export type ILoopJobInput = Partial<
  Pick<
    ILoopJob,
    "slug" | "name" | "public_name" | "public_description" | "prompt" | "min_role" | "enabled" | "rrule" | "tzid"
  >
>;

export interface ILoopTargetRow {
  id: string;
  workspace_slug: string | null;
  user_email: string | null;
  next_run_at: string | null;
  last_skipped_at: string | null;
  last_skip_reason: string;
  last_run: {
    status: string;
    error_code: string;
    model_used: string;
    total_tokens: number | null;
    completed_at: string | null;
  } | null;
}

export interface ILoopTargetsPage {
  page: number;
  results: ILoopTargetRow[];
}
