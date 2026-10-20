// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure rules for the project-settings General tab
// (parity/specs/project-settings-general.md §PSET-005–009). Render-free so
// the form, the write payloads and the unit suites share one answer: field
// limits and messages, the short-code shaper, the visibility and executor
// options, the automated-run policy math, the emblem value shapes and the
// parsed failure codes.

import { isApiError, type ProjectSettingsDetail, type ProjectUpdateInput } from "@pidash/api-client";

/** Longest project name the server stores (PSET-006). */
export const GENERAL_NAME_MAX_LENGTH = 255;
/** Longest short code; typed input is stripped to alphanumerics and capped. */
export const GENERAL_IDENTIFIER_MAX_LENGTH = 10;
/** Longest base branch; the server also restricts its characters. */
export const GENERAL_BASE_BRANCH_MAX_LENGTH = 128;
/** Longest repository URL the input accepts (binding enforces it too). */
export const GENERAL_REPO_URL_MAX_LENGTH = 512;

/** Base branch charset: letters, digits, dot, underscore, slash and dash. */
const BASE_BRANCH_CHARS = /^[A-Za-z0-9._/-]*$/;
/** Alphanumerics of any script; the server uppercases the stored code. */
const NON_ALPHANUMERIC = /[^\p{L}\p{N}]/gu;

/** Guidance text the description field shows while empty (target main#503). */
export const DESCRIPTION_PLACEHOLDER = "Project Context (Will be shown to AI agents)";

/** Field-level messages the form renders inline. */
export const NAME_REQUIRED_MESSAGE = "A project name is required";
export const NAME_TOO_LONG_MESSAGE = `Keep the name to ${GENERAL_NAME_MAX_LENGTH} characters`;
export const IDENTIFIER_REQUIRED_MESSAGE = "A short code is required";
export const IDENTIFIER_TOO_LONG_MESSAGE = `Keep the short code to ${GENERAL_IDENTIFIER_MAX_LENGTH} characters`;
export const IDENTIFIER_UNAVAILABLE_MESSAGE = "That short code is already reserved by another project.";
export const IDENTIFIER_CHECK_FAILED_MESSAGE = "That short code could not be checked. Try again.";
export const BASE_BRANCH_TOO_LONG_MESSAGE = `Keep the base branch to ${GENERAL_BASE_BRANCH_MAX_LENGTH} characters`;
export const BASE_BRANCH_CHARS_MESSAGE = "A branch name may contain only letters, numbers, and . _ / -";
export const BUDGET_REQUIRED_MESSAGE = "Enter a run budget, or tick no cap";
export const BUDGET_BELOW_ONE_MESSAGE = "The run budget must be at least one";

/** Write-time conflict messages, keyed by the server's own error codes. */
export const NAME_TAKEN_CODE = "PROJECT_NAME_ALREADY_EXIST";
export const IDENTIFIER_TAKEN_CODE = "PROJECT_IDENTIFIER_ALREADY_EXIST";
export const NAME_TAKEN_MESSAGE = "The project name is already taken.";
export const IDENTIFIER_TAKEN_MESSAGE = "The project identifier is already taken.";

/** Emblem file rules: images only, up to five megabytes. */
export const EMBLEM_MIME_TYPES: readonly string[] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
export const EMBLEM_MAX_BYTES = 5 * 1024 * 1024;
export const EMBLEM_TYPE_MESSAGE = "Choose a PNG, JPEG, WebP or GIF image.";
export const EMBLEM_SIZE_MESSAGE = "Keep the emblem under 5 MB.";

/** Empty repository input is refused client-side, with no request (PSET-008). */
export const REPO_EMPTY_TITLE = "Error!";
export const REPO_EMPTY_MESSAGE = "Enter a Git repository URL first.";

/** Visibility choices the server reads: 0 private, 2 public. */
export const VISIBILITY_OPTIONS = [
  { value: 0, label: "Private" },
  { value: 2, label: "Public" },
] as const;

/** The three executor kinds, in picker order, with their visible labels. */
export const EXECUTOR_OPTIONS = [
  { kind: "local_runner", label: "Local runner" },
  { kind: "cloud_agent", label: "Cloud agent" },
  { kind: "managed_runner", label: "Desktop agent" },
] as const;

/** Default run pool when the stored budget is uncapped and none was typed. */
export const DEFAULT_MAX_TICKS = 10;
/** The server's no-cap sentinel for the run pool. */
export const INFINITE_MAX_TICKS = -1;

/** Cadence presets from half an hour through a day (PSET-009). */
export const CADENCE_OPTIONS = [
  { seconds: 1_800, label: "30 minutes" },
  { seconds: 3_600, label: "1 hour" },
  { seconds: 10_800, label: "3 hours" },
  { seconds: 21_600, label: "6 hours" },
  { seconds: 43_200, label: "12 hours" },
  { seconds: 86_400, label: "24 hours" },
] as const;

/** Marker the picker shows for a rhythm outside the presets. */
export const CUSTOM_CADENCE_VALUE = "custom";
export const CUSTOM_CADENCE_LABEL = "Custom";

/** Shape the tab renders; every editable field is a string. */
export interface GeneralFormValues {
  name: string;
  identifier: string;
  description: string;
  network: number;
  timezone: string;
  baseBranch: string;
  repoUrl: string;
  logoProps: Record<string, unknown>;
  executor: string;
  ticking: boolean;
  budget: string;
  noCap: boolean;
  /** True while the budget field is blank or uncapped; kept for restore. */
  lastCappedBudget: string;
  cadence: string;
}

export type GeneralField = "name" | "identifier" | "baseBranch" | "budget";
export type GeneralFieldErrors = Partial<Record<GeneralField, string>>;

/** The stored budget when it is a real cap, else null (uncapped/unset). */
export function cappedBudgetOf(project: Pick<ProjectSettingsDetail, "agent_default_max_ticks">): number | null {
  return project.agent_default_max_ticks > 0 ? project.agent_default_max_ticks : null;
}

/** Prefill the form from the stored row. */
export function generalFormFrom(project: ProjectSettingsDetail): GeneralFormValues {
  const capped = cappedBudgetOf(project);
  return {
    name: project.name,
    identifier: project.identifier,
    description: project.description,
    network: project.network === 2 ? 2 : 0,
    timezone: project.timezone,
    baseBranch: project.base_branch,
    repoUrl: project.repo_url,
    logoProps: project.logo_props,
    executor: project.default_agent_executor === "" ? "local_runner" : project.default_agent_executor,
    ticking: project.agent_ticking_enabled,
    budget: capped === null ? "" : String(capped),
    noCap: capped === null,
    lastCappedBudget: String(capped ?? DEFAULT_MAX_TICKS),
    cadence: selectCadence(project.agent_default_interval_seconds),
  };
}

/**
 * Re-seat the form on a freshly saved row while keeping the cap the
 * operator typed. A no-cap save stores the `-1` sentinel, so the row alone
 * cannot say what the budget was before the tick; the typed value is the
 * only memory of it and unticking must restore it (PSET-009).
 */
export function reseatGeneralForm(
  saved: ProjectSettingsDetail,
  previous: Pick<GeneralFormValues, "lastCappedBudget">
): GeneralFormValues {
  const next = generalFormFrom(saved);
  if (!next.noCap) return next;
  const typed = previous.lastCappedBudget.trim();
  return typed === "" || Number(typed) < 1 ? next : { ...next, lastCappedBudget: typed };
}

/** Which cadence option a stored rhythm is; an out-of-band value is custom. */
export function selectCadence(seconds: number): string {
  if (CADENCE_OPTIONS.some((option) => option.seconds === seconds)) return String(seconds);
  return CUSTOM_CADENCE_VALUE;
}

/** Visible label for the current cadence choice. */
export function cadenceLabel(choice: string): string {
  const preset = CADENCE_OPTIONS.find((option) => String(option.seconds) === choice);
  return preset?.label ?? CUSTOM_CADENCE_LABEL;
}

/** Short code as typed: non-alphanumerics dropped, uppercased, length-capped. */
export function shapeIdentifier(value: string): string {
  return value.replace(NON_ALPHANUMERIC, "").toUpperCase().slice(0, GENERAL_IDENTIFIER_MAX_LENGTH);
}

/** Whole, non-negative budget characters only; empty stays empty. */
export function shapeBudget(value: string): string {
  return value.replace(/[^\d]/g, "");
}

/**
 * The emoji's stored form: its code points in decimal, joined by dashes
 * (the oracle pins "128640" for a rocket). The mark decoder accepts the
 * literal character too, but the stored row names the code points.
 */
export function unifiedEmoji(emoji: string): string {
  return [...emoji]
    .map((part) => part.codePointAt(0))
    .filter((point): point is number => point !== undefined)
    .join("-");
}

/**
 * The logo value the emoji/icon picker writes. Emoji marks carry the
 * stored `{ value }` record; icon marks carry their name and color.
 */
export function markEmblem(type: "emoji" | "icon", value: unknown): Record<string, unknown> {
  if (type === "emoji") {
    return { in_use: "emoji", emoji: { value: typeof value === "string" ? unifiedEmoji(value) : value } };
  }
  return { in_use: "icon", icon: value };
}

/** The logo value the upload control writes: an uploaded-mark record. */
export function uploadedEmblem(url: string): Record<string, unknown> {
  return { in_use: "upload", url };
}

/** Reject a picked file that is not an accepted image. */
export function emblemFileError(file: Pick<File, "type" | "size">): string | null {
  if (!EMBLEM_MIME_TYPES.includes(file.type)) return EMBLEM_TYPE_MESSAGE;
  if (file.size > EMBLEM_MAX_BYTES) return EMBLEM_SIZE_MESSAGE;
  return null;
}

/**
 * Client-side field rules. The identifier's availability is checked
 * separately (and is advisory); identifier length counts shaped input.
 */
export function validateGeneralFields(values: GeneralFormValues): GeneralFieldErrors {
  const errors: GeneralFieldErrors = {};
  if (values.name.trim() === "") {
    errors.name = NAME_REQUIRED_MESSAGE;
  } else if (values.name.length > GENERAL_NAME_MAX_LENGTH) {
    errors.name = NAME_TOO_LONG_MESSAGE;
  }
  if (values.identifier.trim() === "") {
    errors.identifier = IDENTIFIER_REQUIRED_MESSAGE;
  } else if (values.identifier.length > GENERAL_IDENTIFIER_MAX_LENGTH) {
    errors.identifier = IDENTIFIER_TOO_LONG_MESSAGE;
  }
  if (values.baseBranch.length > GENERAL_BASE_BRANCH_MAX_LENGTH) {
    errors.baseBranch = BASE_BRANCH_TOO_LONG_MESSAGE;
  } else if (!BASE_BRANCH_CHARS.test(values.baseBranch)) {
    errors.baseBranch = BASE_BRANCH_CHARS_MESSAGE;
  }
  // Nothing validates while the clock is off (PSET-009).
  if (values.ticking && !values.noCap) {
    if (values.budget.trim() === "") errors.budget = BUDGET_REQUIRED_MESSAGE;
    else if (Number(values.budget) < 1) errors.budget = BUDGET_BELOW_ONE_MESSAGE;
  }
  return errors;
}

/** The budget value a save writes: the no-cap sentinel, the typed cap, or the default. */
export function budgetOnSave(values: GeneralFormValues): number {
  if (values.noCap) return INFINITE_MAX_TICKS;
  const trimmed = values.budget.trim();
  if (trimmed === "") return DEFAULT_MAX_TICKS;
  return Math.max(1, Number(trimmed));
}

export interface BuildPayloadInput {
  values: GeneralFormValues;
  /** True when the cadence picker moved in this form session. */
  cadenceDirty: boolean;
}

/**
 * The project PATCH body (PSET-006/007/009). It never carries the
 * repository URL: binding owns that field (PSET-008). Per-stage cadence
 * columns ride along only when the picker moved, so a rhythm set elsewhere
 * survives an unrelated save.
 */
export function buildGeneralPayload({ values, cadenceDirty }: BuildPayloadInput): ProjectUpdateInput {
  const payload: ProjectUpdateInput = {
    name: values.name,
    identifier: values.identifier,
    description: values.description,
    network: values.network,
    timezone: values.timezone,
    base_branch: values.baseBranch,
    logo_props: values.logoProps,
    default_agent_executor: values.executor,
    agent_ticking_enabled: values.ticking,
    agent_default_max_ticks: budgetOnSave(values),
  };
  // The custom marker is a caption, never a rhythm: a save writes the
  // stage columns only for a real preset.
  const seconds = cadenceSeconds(values.cadence);
  if (cadenceDirty && seconds !== null) {
    payload.agent_default_interval_seconds = seconds;
    payload.agent_review_default_interval_seconds = seconds;
    payload.agent_test_default_interval_seconds = seconds;
  }
  return payload;
}

/** A cadence choice as whole seconds, or null for the custom marker. */
export function cadenceSeconds(choice: string): number | null {
  if (choice === CUSTOM_CADENCE_VALUE) return null;
  const seconds = Number(choice);
  return Number.isFinite(seconds) && seconds > 0 ? seconds : null;
}

/** Availability precheck answer → field error, or null when the name is free. */
export function identifierUnavailable(check: { exists: number }): string | null {
  return check.exists > 0 ? IDENTIFIER_UNAVAILABLE_MESSAGE : null;
}

/** One write-time conflict from the server's field codes, or null. */
export function generalConflictError(error: unknown): { field: GeneralField; message: string } | null {
  if (!isApiError(error) || error.fields === undefined) return null;
  if (error.fields["name"]?.includes(NAME_TAKEN_CODE)) {
    return { field: "name", message: NAME_TAKEN_MESSAGE };
  }
  if (error.fields["identifier"]?.includes(IDENTIFIER_TAKEN_CODE)) {
    return { field: "identifier", message: IDENTIFIER_TAKEN_MESSAGE };
  }
  return null;
}

/** Text a non-conflict save failure shows under the notice. */
export function saveFailureMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The URL shapes a binding answer can carry (spec §PSET-008). */
interface RepositoryStatusUrls {
  repo_url?: string;
  repository?: { web_url?: string; clone_url_http?: string };
}

/**
 * The canonical URL the project stores, whichever answer shape carried it.
 * The bind answer's `repo_url` is the canonical clone URL the server writes
 * onto the project; the status GET has no top-level `repo_url`, so it is
 * built from `repository.clone_url_http` (not `web_url`, which drops the
 * `.git` suffix the stored value keeps).
 */
export function canonicalRepositoryUrl(status: RepositoryStatusUrls | undefined): string | undefined {
  if (status === undefined) return undefined;
  if (typeof status.repo_url === "string" && status.repo_url.trim() !== "") return status.repo_url;
  const cloneUrl = status.repository?.clone_url_http;
  if (typeof cloneUrl === "string" && cloneUrl.trim() !== "") return cloneUrl;
  const webUrl = status.repository?.web_url;
  return typeof webUrl === "string" ? webUrl : undefined;
}

/** Repository bind control state: a bound pill only on an exact URL match. */
export function repositoryBoundState(
  status: (RepositoryStatusUrls & { bound: boolean }) | undefined,
  typed: string
): "bound" | "bind" {
  if (status?.bound !== true) return "bind";
  const trimmed = typed.trim();
  if (trimmed === "") return "bind";
  const canonical = canonicalRepositoryUrl(status);
  if (canonical === undefined) return "bind";
  return canonical !== "" && trimmed === canonical ? "bound" : "bind";
}

/** Executor choices with availability; the local runner is always enabled. */
export function executorChoices(
  options: ReadonlyArray<{ kind: string; available: boolean }>
): Array<{ kind: string; label: string; disabled: boolean }> {
  return EXECUTOR_OPTIONS.map((option) => ({
    kind: option.kind,
    label: option.label,
    disabled:
      option.kind === "local_runner" ? false : !options.some((row) => row.kind === option.kind && row.available),
  }));
}
