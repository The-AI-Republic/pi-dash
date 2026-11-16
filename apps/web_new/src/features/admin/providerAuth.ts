// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider sub-page registry and shared form behavior (ADM-027,
// ADM-032–035, from parity/specs/admin.md §4.5, §4.10–4.13): the four
// OAuth provider definitions with their pinned identity strings, the
// origin/callback derivation, the sync-flag codec, and the pure form
// state helpers (defaults, dirtiness, silent required gating, payload
// assembly, reset from the server echo). Notice and label wording is
// pinned by the parity scenarios; the structure is fresh.
import type { InstanceConfigurationList } from "@pidash/api-client";
import type { ReactNode } from "react";

import { GOD_MODE_MOUNT } from "./adminShell.js";
import type { ConfigMap } from "./authMethods.js";

export type AuthProviderSlug = "github" | "gitlab" | "gitea" | "google";

export interface AuthProviderDef {
  /** Route slug under /god-mode/authentication/. */
  slug: AuthProviderSlug;
  /** Display name, pinned by the parity scenario. */
  name: string;
  /** Shell card description, pinned by the parity scenario. */
  description: string;
  /** Tab title, pinned by the parity scenario. */
  title: string;
  /** The master enable flag in the configurations map. */
  enableKey: string;
  /** The per-provider sync string flag. */
  syncKey: string;
  /** Sync switch caption, pinned by the parity scenario. */
  syncLabel: string;
  /** GitHub and Google show an origin copy field; GitLab and Gitea do not. */
  showsOriginCopy: boolean;
  /** Callback copy label: "Callback URL" on GitLab, "Callback URI" elsewhere. */
  callbackLabel: "Callback URI" | "Callback URL";
}

const GITHUB_PROVIDER: AuthProviderDef = {
  slug: "github",
  name: "GitHub",
  description: "Allow members to login or sign up to pi dash with their GitHub accounts.",
  title: "GitHub Authentication - God Mode",
  enableKey: "IS_GITHUB_ENABLED",
  syncKey: "ENABLE_GITHUB_SYNC",
  syncLabel: "Refresh user attributes from GitHub during sign in",
  showsOriginCopy: true,
  callbackLabel: "Callback URI",
};

const GITLAB_PROVIDER: AuthProviderDef = {
  slug: "gitlab",
  name: "GitLab",
  description: "Allow members to login or sign up to pi dash with their GitLab accounts.",
  title: "GitLab Authentication - God Mode",
  enableKey: "IS_GITLAB_ENABLED",
  syncKey: "ENABLE_GITLAB_SYNC",
  syncLabel: "Refresh user attributes from GitLab during sign in",
  showsOriginCopy: false,
  callbackLabel: "Callback URL",
};

const GITEA_PROVIDER: AuthProviderDef = {
  slug: "gitea",
  name: "Gitea",
  description: "Allow members to login or sign up to pi dash with their Gitea accounts.",
  title: "Gitea Authentication - God Mode",
  enableKey: "IS_GITEA_ENABLED",
  syncKey: "ENABLE_GITEA_SYNC",
  syncLabel: "Refresh user attributes from Gitea during sign in",
  showsOriginCopy: false,
  callbackLabel: "Callback URI",
};

const GOOGLE_PROVIDER: AuthProviderDef = {
  slug: "google",
  name: "Google",
  description: "Allow members to login or sign up to pi dash with their Google accounts.",
  title: "Google Authentication - God Mode",
  enableKey: "IS_GOOGLE_ENABLED",
  syncKey: "ENABLE_GOOGLE_SYNC",
  syncLabel: "Refresh user attributes from Google during sign in",
  showsOriginCopy: true,
  callbackLabel: "Callback URI",
};

export const AUTH_PROVIDERS: readonly AuthProviderDef[] = [
  GITHUB_PROVIDER,
  GITLAB_PROVIDER,
  GITEA_PROVIDER,
  GOOGLE_PROVIDER,
];

export const AUTH_PROVIDER_BY_SLUG: Record<AuthProviderSlug, AuthProviderDef> = {
  github: GITHUB_PROVIDER,
  gitlab: GITLAB_PROVIDER,
  gitea: GITEA_PROVIDER,
  google: GOOGLE_PROVIDER,
};

/** Body skeleton bars while configuration loads, pinned by the parity scenario. */
export const AUTH_PROVIDER_SKELETON_BARS = 5;

/** Where the back action and the discard modal leave to. */
export const AUTH_PROVIDER_LANDING_HREF = `${GOD_MODE_MOUNT}/authentication/`;

export const PROVIDER_MASTER_SAVED_TITLE = "Configuration saved";

/** Master-toggle success message for the flip direction, pinned by the parity scenario. */
export function providerMasterSavedMessage(name: string, enabled: boolean): string {
  return `${name} authentication is now ${enabled ? "active" : "disabled"}.`;
}

export const PROVIDER_MASTER_FAILED_TITLE = "Error";
export const PROVIDER_MASTER_FAILED_MESSAGE = "Failed to save configuration";

export const PROVIDER_SAVE_DONE_TITLE = "Done!";

/** Credential-save success message, pinned by the parity scenario. */
export function providerSaveDoneMessage(name: string): string {
  return `Your ${name} authentication is configured. You should test it now.`;
}

export const PROVIDER_SAVE_IDLE_LABEL = "Save changes";
export const PROVIDER_SAVE_BUSY_LABEL = "Saving";

export const PROVIDER_DISCARD_TITLE = "You have unsaved changes";
export const PROVIDER_DISCARD_BODY = "Changes you made will be lost if you go back. Do you wish to go back?";
export const PROVIDER_DISCARD_KEEP_LABEL = "Keep editing";
export const PROVIDER_BACK_LABEL = "Go back";

export const PROVIDER_COPY_DONE_TITLE = "Copied to clipboard";

/** Copy-field notice message for the clicked label, pinned by the parity scenario. */
export function providerCopyDoneMessage(label: string): string {
  return `The ${label} has been successfully copied to your clipboard`;
}

/** One credential input the shared form renders (field sets arrive in NEWFRONT-511). */
export interface ProviderCredentialField {
  /** Configuration key; also the input id. */
  key: string;
  label: string;
  type: "text" | "password";
  required: boolean;
  placeholder?: string | undefined;
  description?: ReactNode | undefined;
}

/** Draft (and default) form state: credential values plus the staged sync switch. */
export interface ProviderFormValues {
  fields: Record<string, string>;
  syncOn: boolean;
}

/** Stored sync flag reads on only as the "1" string; missing or blank reads off. */
export function decodeSyncFlag(value: string | null | undefined): boolean {
  return value === "1";
}

/** Staged sync switch value as the stored string flag. */
export function encodeSyncFlag(on: boolean): "1" | "0" {
  return on ? "1" : "0";
}

/** Credential draft read; keys the draft never carried read blank. */
export function draftValue(values: Record<string, string>, key: string): string {
  return values[key] ?? "";
}

/**
 * Defaults from stored state: each credential reads its stored value
 * (blank when unset) through the optional prefill, and the sync switch
 * reads the stored flag (off when unset).
 */
export function buildProviderFormDefaults(
  config: ConfigMap,
  fields: readonly ProviderCredentialField[],
  syncKey: string,
  prefill: (key: string, stored: string) => string = (_key, stored) => stored
): ProviderFormValues {
  return {
    fields: Object.fromEntries(fields.map((field) => [field.key, prefill(field.key, config[field.key] ?? "")])),
    syncOn: decodeSyncFlag(config[syncKey]),
  };
}

/** Whether the draft differs from the defaults in any credential or the sync switch. */
export function isProviderFormDirty(draft: ProviderFormValues, defaults: ProviderFormValues): boolean {
  if (draft.syncOn !== defaults.syncOn) return true;
  const keys = new Set([...Object.keys(draft.fields), ...Object.keys(defaults.fields)]);
  for (const key of keys) {
    if ((draft.fields[key] ?? "") !== (defaults.fields[key] ?? "")) return true;
  }
  return false;
}

/** Whether every required credential holds a value (unchecked silently blocks the save). */
export function isProviderFormSubmittable(
  values: Record<string, string>,
  fields: readonly ProviderCredentialField[]
): boolean {
  return fields.every((field) => !field.required || (values[field.key] ?? "") !== "");
}

/** Save payload: every credential plus the staged sync string flag. */
export function buildProviderSavePayload(
  values: Record<string, string>,
  syncOn: boolean,
  syncKey: string
): Record<string, string> {
  return { ...values, [syncKey]: encodeSyncFlag(syncOn) };
}

/**
 * Post-save values: each submitted key re-read from the server echo so
 * the form shows stored state, falling back to the submitted value when
 * the echo omits a key.
 */
export function readProviderSavedValues(
  response: InstanceConfigurationList,
  submitted: Record<string, string>
): Record<string, string> {
  const echoed = new Map(response.map((row) => [row.key, row.value]));
  return Object.fromEntries(Object.entries(submitted).map(([key, value]) => [key, echoed.get(key) ?? value]));
}

/**
 * Base origin for the copy-out URLs: the configured API base, or the
 * current window origin when unconfigured (blank in non-DOM runtimes).
 */
export function resolveAuthOrigin(apiBaseUrl: string): string {
  const trimmed = apiBaseUrl.replace(/\/+$/, "");
  if (trimmed !== "") return trimmed;
  if (typeof window !== "undefined") return window.location.origin;
  return "";
}

/** Callback URL for the provider, pinned to the <origin>/auth/<provider>/callback/ shape. */
export function buildAuthCallbackUrl(origin: string, slug: AuthProviderSlug): string {
  return `${origin.replace(/\/+$/, "")}/auth/${slug}/callback/`;
}

export interface ProviderCopyField {
  label: string;
  url: string;
}

/**
 * Copy-out fields for the provider: origin plus callback where shown,
 * callback only on GitLab and Gitea. Both URLs derive from the same
 * normalized base so the callback always matches the shown origin.
 */
export function buildProviderCopyFields(provider: AuthProviderDef, origin: string): readonly ProviderCopyField[] {
  const base = origin.replace(/\/+$/, "");
  const callback: ProviderCopyField = {
    label: provider.callbackLabel,
    url: `${base}/auth/${provider.slug}/callback/`,
  };
  if (!provider.showsOriginCopy) return [callback];
  return [{ label: "Origin URL", url: base }, callback];
}

/**
 * Copy-field help link target: static, or derived from the live
 * credential draft (the Gitea application-settings link follows the
 * typed host in NEWFRONT-511).
 */
export type CopyHelpHref = string | ((fieldValues: Record<string, string>) => string);

/** Resolve the help target for the current draft; null when the field carries no link. */
export function resolveCopyHelpHref(
  href: CopyHelpHref | undefined,
  fieldValues: Record<string, string>
): string | null {
  if (href === undefined) return null;
  if (typeof href === "string") return href;
  return href(fieldValues);
}
