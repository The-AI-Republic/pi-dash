// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth-method registry and configuration helpers (ADM-024, ADM-025,
// ADM-026, from parity/specs/admin.md §4.2–4.4): the six method cards
// in their fixed order, the string-flag reads over the configurations
// map, the last-method-disable guard, and the OAuth configured check.
// Card names, descriptions, and the guard scope are pinned by the
// parity scenarios; everything else here is fresh structure.
import type { InstanceConfigurationList } from "@pidash/api-client";

/** Own method ids in registry order; testids derive from these. */
export type AuthMethodKey = "unique-codes" | "passwords" | "google" | "github" | "gitlab" | "gitea";

export interface AuthMethodDef {
  key: AuthMethodKey;
  /** Display name, pinned by the parity scenario. */
  name: string;
  /** Card description, pinned by the parity scenario. */
  description: string;
  /** The per-method enable flag in the configurations map. */
  enableKey: string;
  /** Toggle-only cards versus OAuth cards with configure/edit states. */
  control: "toggle" | "oauth";
  /** Provider page slug for the configure/edit href; absent when toggle-only. */
  providerSlug?: string | undefined;
  /** Keys that must all hold values for an OAuth card to read configured. */
  credentialKeys: readonly string[];
}

/**
 * Method cards in fixed render order: email magic-link codes,
 * passwords, Google, GitHub, GitLab, Gitea. Names and descriptions
 * match the parity scenario exactly, including its lowercase
 * "pi dash" in the GitLab and Gitea lines.
 */
export const AUTH_METHODS: readonly AuthMethodDef[] = [
  {
    key: "unique-codes",
    name: "Unique codes",
    description:
      "Log in or sign up for Pi Dash using codes sent via email. You need to have set up SMTP to use this method.",
    enableKey: "ENABLE_MAGIC_LINK_LOGIN",
    control: "toggle",
    credentialKeys: [],
  },
  {
    key: "passwords",
    name: "Passwords",
    description: "Allow members to create accounts with passwords and use it with their email addresses to sign in.",
    enableKey: "ENABLE_EMAIL_PASSWORD",
    control: "toggle",
    credentialKeys: [],
  },
  {
    key: "google",
    name: "Google",
    description: "Allow members to log in or sign up for Pi Dash with their Google accounts.",
    enableKey: "IS_GOOGLE_ENABLED",
    control: "oauth",
    providerSlug: "google",
    credentialKeys: ["GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET"],
  },
  {
    key: "github",
    name: "GitHub",
    description: "Allow members to log in or sign up for Pi Dash with their GitHub accounts.",
    enableKey: "IS_GITHUB_ENABLED",
    control: "oauth",
    providerSlug: "github",
    credentialKeys: ["GITHUB_CLIENT_ID", "GITHUB_CLIENT_SECRET"],
  },
  {
    key: "gitlab",
    name: "GitLab",
    description: "Allow members to log in or sign up to pi dash with their GitLab accounts.",
    enableKey: "IS_GITLAB_ENABLED",
    control: "oauth",
    providerSlug: "gitlab",
    credentialKeys: ["GITLAB_CLIENT_ID", "GITLAB_CLIENT_SECRET"],
  },
  {
    key: "gitea",
    name: "Gitea",
    description: "Allow members to log in or sign up to pi dash with their Gitea accounts.",
    enableKey: "IS_GITEA_ENABLED",
    control: "oauth",
    providerSlug: "gitea",
    credentialKeys: ["GITEA_HOST", "GITEA_CLIENT_ID", "GITEA_CLIENT_SECRET"],
  },
];

/** The six method enable flags the last-method guard counts. */
export const AUTH_METHOD_ENABLE_KEYS: readonly string[] = AUTH_METHODS.map((method) => method.enableKey);

/** Open-sign-up flag: a switch of its own, never a guarded method key. */
export const ENABLE_SIGNUP_KEY = "ENABLE_SIGNUP";

/** Configurations map: key to stored string (flags are "1"/"0", blanks read off). */
export type ConfigMap = Readonly<Record<string, string | null | undefined>>;

/** Index the fetched rows by key for flag and credential reads. */
export function toConfigMap(rows: InstanceConfigurationList): ConfigMap {
  return Object.fromEntries(rows.map((row) => [row.key, row.value]));
}

/** A stored flag reads on only as the "1" string; missing, blank, or null reads off. */
export function isConfigFlagOn(value: string | null | undefined): boolean {
  return value === "1";
}

/** Whether every required credential key holds a value (toggle-only cards carry none). */
export function isMethodConfigured(method: AuthMethodDef, config: ConfigMap): boolean {
  return method.credentialKeys.every((key) => (config[key] ?? "") !== "");
}

/**
 * Whether switching `enableKey` off is allowed. Refused only when the
 * key is a method flag, currently on, and the lone enabled method —
 * keys outside the six (open-sign-up) and already-off flags flip
 * freely.
 */
export function canDisableAuthMethod(enableKey: string, config: ConfigMap): boolean {
  if (!AUTH_METHOD_ENABLE_KEYS.includes(enableKey)) return true;
  if (!isConfigFlagOn(config[enableKey])) return true;
  const enabledCount = AUTH_METHOD_ENABLE_KEYS.filter((key) => isConfigFlagOn(config[key])).length;
  return enabledCount > 1;
}
