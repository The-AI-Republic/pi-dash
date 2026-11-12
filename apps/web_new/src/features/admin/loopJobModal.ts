// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job create/edit modal pure helpers (ADM-055/056/057, from
// parity/specs/admin.md §7.5–7.7): create defaults, the save gate,
// create/patch payload builders, and the save-failure message
// resolver. Strings marked oracle-pinned are asserted verbatim by
// e2e/parity/admin/loop.spec.ts and must stay exact; field labels
// follow the inventory row wording.
import type { LoopJob, LoopJobCreateInput, LoopJobPatchInput } from "@pidash/api-client";

/** Create-modal title (oracle-pinned: loop.spec.ts ADM-055). */
export const LOOP_JOB_MODAL_CREATE_TITLE = "New job";

/** Edit-modal title (oracle-pinned: loop.spec.ts ADM-056). */
export const LOOP_JOB_MODAL_EDIT_TITLE = "Edit job";

/** Save-failure notice title (oracle-pinned: loop.spec.ts ADM-057). */
export const LOOP_JOB_SAVE_FAILED_TITLE = "Save failed";

/**
 * Save-failure notice message when the failure carries no server
 * text (oracle-pinned: loop.spec.ts ADM-057 names this fallback).
 */
export const LOOP_JOB_SAVE_FALLBACK_MESSAGE = "Check the fields and try again.";

/** Create default for the minimum-role select: member (spec §7.5). */
export const LOOP_JOB_DEFAULT_MIN_ROLE = 15;

/** Create default for the timezone field (spec §7.5). */
export const LOOP_JOB_DEFAULT_TZID = "UTC";

/**
 * Create default for the recurrence field: the daily 03:00 UTC rule
 * (oracle-pinned value: loop.spec.ts ADM-055).
 */
export const LOOP_JOB_DEFAULT_RRULE = "FREQ=DAILY;BYHOUR=3;BYMINUTE=0";

/** Modal field labels, top to bottom (inventory ADM-055 wording). */
export const LOOP_JOB_FIELD_LABELS = {
  slug: "Slug",
  name: "Admin name",
  publicName: "User-facing name",
  publicDescription: "User-facing description",
  prompt: "Prompt",
  minRole: "Min role",
  tzid: "Timezone",
  rrule: "Recurrence rule",
} as const;

/** Every field the modal edits, in payload order. */
export interface LoopJobFormFields {
  slug: string;
  name: string;
  publicName: string;
  publicDescription: string;
  prompt: string;
  minRole: number;
  tzid: string;
  rrule: string;
}

/**
 * Blank-text create state (spec §7.5): identity fields empty, role at
 * member, timezone at UTC, recurrence at the daily rule.
 */
export const LOOP_JOB_FORM_DEFAULTS: LoopJobFormFields = {
  slug: "",
  name: "",
  publicName: "",
  publicDescription: "",
  prompt: "",
  minRole: LOOP_JOB_DEFAULT_MIN_ROLE,
  tzid: LOOP_JOB_DEFAULT_TZID,
  rrule: LOOP_JOB_DEFAULT_RRULE,
};

/** Pre-fill every field from the stored job (spec §7.6). */
export function loopJobFormFromJob(job: LoopJob): LoopJobFormFields {
  return {
    slug: job.slug,
    name: job.name,
    publicName: job.public_name,
    publicDescription: job.public_description,
    prompt: job.prompt,
    minRole: job.min_role,
    tzid: job.tzid,
    rrule: job.rrule,
  };
}

function isNonBlank(value: string): boolean {
  return value.trim() !== "";
}

/**
 * Save gate (spec §7.7): enabled exactly when slug, admin name,
 * user-facing name, and prompt are all non-blank. Description, role,
 * timezone, and recurrence carry defaults and never gate.
 */
export function isLoopJobSaveEnabled(fields: LoopJobFormFields): boolean {
  return (
    isNonBlank(fields.slug) && isNonBlank(fields.name) && isNonBlank(fields.publicName) && isNonBlank(fields.prompt)
  );
}

/**
 * Create body (spec §7.5): the eight modal fields under their wire
 * names. No enabled flag — the server default owns it.
 */
export function buildLoopJobCreateInput(fields: LoopJobFormFields): LoopJobCreateInput {
  return {
    slug: fields.slug,
    name: fields.name,
    public_name: fields.publicName,
    public_description: fields.publicDescription,
    prompt: fields.prompt,
    min_role: fields.minRole,
    rrule: fields.rrule,
    tzid: fields.tzid,
  };
}

/**
 * Edit body (spec §7.6): the same eight fields. The slug rides along
 * unchanged for builtin jobs (its field is locked, not dropped), and
 * enabled stays out — the list toggle owns it.
 */
export function buildLoopJobPatchInput(fields: LoopJobFormFields): LoopJobPatchInput {
  return {
    slug: fields.slug,
    name: fields.name,
    public_name: fields.publicName,
    public_description: fields.publicDescription,
    prompt: fields.prompt,
    min_role: fields.minRole,
    rrule: fields.rrule,
    tzid: fields.tzid,
  };
}

/** The slug field locks exactly for builtin jobs (spec §7.6). */
export function isLoopJobSlugLocked(job: LoopJob | undefined): boolean {
  return job?.is_builtin ?? false;
}

/**
 * Save-failure notice message (spec §7.7): the server-provided text
 * first, the generic fallback when the failure carries none. The API
 * client already folds the server detail and error fields into the
 * error message, so a non-blank message is the server text.
 */
export function resolveLoopJobSaveError(error: unknown): string {
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  return LOOP_JOB_SAVE_FALLBACK_MESSAGE;
}
