// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connect mechanics (written from parity/specs/cloud.md §6,
// CLOUD-024/025/026/027/031): mechanism classification, the
// synchronously reserved handoff tab, return-path and outcome-param
// handling, hub-address building with its safety gate, and the
// credential-dialog field rules (manifest fields, validation,
// trimming, secret masking). Pure except the tab reservation itself.
import type { OpenhubAuthStatus, OpenhubCredentialField } from "@pidash/api-client";

/** How one app connects: provider tab, credential dialog, or nothing. */
export type AppMechanism = "provider" | "key" | "none";

/**
 * Classify catalog or auth metadata. Key and none spellings are exact;
 * anything else — including an omitted mechanism on older catalogs —
 * reads as provider, which reserves the handoff tab (CLOUD-025).
 */
export function appMechanism(authType: string | undefined): AppMechanism {
  if (authType === "api_key") return "key";
  if (authType === "none") return "none";
  return "provider";
}

/** Placeholder tab title while the handoff address loads (CLOUD-025). */
export const CONNECT_TAB_TITLE = "Connecting…";

/**
 * Reserve the handoff tab synchronously during the click (CLOUD-025):
 * placeholder content the flow later steers (OAuth begin address or
 * hub detail) or closes. No `noopener` feature: that makes window.open
 * return null, so the flow could never steer or close the tab it just
 * opened. The opener is severed explicitly instead, while the
 * same-origin placeholder is still scriptable — the retained handle
 * steers and closes, but the provider page cannot reach back
 * (NEWFRONT-662). Null when the popup blocker eats the tab or the
 * window is already gone — the flow reports instead of failing
 * silently.
 */
export function reserveConnectTab(opener: Window): Window | null {
  const tab = opener.open("about:blank", "_blank");
  if (tab !== null && !tab.closed) {
    try {
      tab.opener = null;
    } catch {
      // A severed-by-policy opener reads null already.
    }
  }
  if (tab === null || tab.closed) return null;
  try {
    tab.document.title = CONNECT_TAB_TITLE;
  } catch {
    // A tab that will not take a title still navigates.
  }
  return tab;
}

/** Round-trip outcome params the bounce leaves behind (CLOUD-026). */
export const CONNECT_ERROR_PARAM = "error";
export const CONNECT_OUTCOME_PARAM = "connected";

/** This page's query string minus the round-trip outcome params. */
export function stripOutcomeParams(search: string): string {
  const params = new URLSearchParams(search);
  params.delete(CONNECT_ERROR_PARAM);
  params.delete(CONNECT_OUTCOME_PARAM);
  const rest = params.toString();
  return rest === "" ? "" : `?${rest}`;
}

/**
 * The consent-handoff return path (CLOUD-025): this page with any
 * previous round-trip outcome stripped. Never the fragment — it would
 * break the handoff address and leak page state to provider logs.
 */
export function connectReturnPath(location: { pathname: string; search: string }): string {
  return `${location.pathname}${stripOutcomeParams(location.search)}`;
}

/** Whether the address carries a connect failure to report (CLOUD-026). */
export function hasBounceError(search: string): boolean {
  return new URLSearchParams(search).has(CONNECT_ERROR_PARAM);
}

/** Whether the address carries outcome params to strip (CLOUD-026). */
export function hasOutcomeParams(search: string): boolean {
  const params = new URLSearchParams(search);
  return params.has(CONNECT_ERROR_PARAM) || params.has(CONNECT_OUTCOME_PARAM);
}

/**
 * The address after reporting a bounced outcome (CLOUD-026): same page
 * and fragment, outcome params gone, everything else kept — so later
 * searches can never re-fire the report.
 */
export function cleanedBounceHref(location: { pathname: string; search: string; hash: string }): string {
  return `${location.pathname}${stripOutcomeParams(location.search)}${location.hash}`;
}

/** Hosts allowed to serve the app hub over plain HTTP (CLOUD-031). */
const LOOPBACK_HOSTS = new Set(["localhost", "127.0.0.1", "::1", "[::1]"]);

/**
 * The hub detail address for an already-connected provider app
 * (CLOUD-031): the status answer's base plus the app's identity
 * (preferred slug, then platform id). Null when the base is refused —
 * missing, unparseable, embedding credentials, or neither plain
 * credentialless HTTPS nor loopback HTTP — and the caller closes the
 * tab and reports instead of navigating.
 */
export function hubAppUrl(hubBase: string | undefined, slug: string, appId: string): string | null {
  if (hubBase === undefined || hubBase === "") return null;
  let base: URL;
  try {
    base = new URL(hubBase);
  } catch {
    return null;
  }
  if (base.username !== "" || base.password !== "") return null;
  const secure = base.protocol === "https:";
  const loopback = base.protocol === "http:" && LOOPBACK_HOSTS.has(base.hostname);
  if (!secure && !loopback) return null;
  const identity = slug !== "" ? slug : appId;
  return `${base.origin}/apps/${encodeURIComponent(identity)}`;
}

/**
 * Issuance links render only for absolute credentialless HTTPS
 * addresses (CLOUD-027): plain HTTP, embedded credentials, and
 * relative paths stay unlinked.
 */
export function isSafeSetupUrl(candidate: string | undefined): candidate is string {
  if (candidate === undefined || candidate === "") return false;
  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    return false;
  }
  return parsed.protocol === "https:" && parsed.username === "" && parsed.password === "";
}

/**
 * Publisher instructions as plain text (CLOUD-027): emphasis markers
 * dropped, everything else untouched for React to escape — markup is
 * never interpreted.
 */
export function cleanInstructions(markdown: string | undefined): string {
  if (markdown === undefined) return "";
  return markdown.replaceAll("**", "");
}

/** Fallback field when a bare key type carries no manifest (CLOUD-027). */
export const FALLBACK_CREDENTIAL_FIELD: OpenhubCredentialField = {
  key: "api_key",
  label: "API key",
  type: "secret",
};

/** Dialog fields: the manifest's, or the single secret fallback. */
export function dialogFields(auth: OpenhubAuthStatus): OpenhubCredentialField[] {
  const manifest = auth.manualSetup?.fields;
  if (manifest !== undefined && manifest.length > 0) return manifest;
  return [FALLBACK_CREDENTIAL_FIELD];
}

/** Whether a field masks its input and clears after submit (CLOUD-027). */
export function isSecretField(field: OpenhubCredentialField): boolean {
  return field.type === "secret";
}

/** A field left empty — every field is required (CLOUD-027). */
export const CREDENTIAL_REQUIRED_ERROR = "Enter a value.";

/** A value failing its format hint (CLOUD-027). */
export const CREDENTIAL_FORMAT_ERROR = "Enter a value in the expected format.";

/**
 * Every problem at once, keyed by field (CLOUD-027): empties after
 * trimming first, then format hints. An invalid hint pattern never
 * blocks the form.
 */
export function validateCredentialValues(
  fields: OpenhubCredentialField[],
  values: Record<string, string>
): Record<string, string> {
  const problems: Record<string, string> = {};
  for (const field of fields) {
    const value = (values[field.key] ?? "").trim();
    if (value === "") {
      problems[field.key] = CREDENTIAL_REQUIRED_ERROR;
      continue;
    }
    if (field.validation === undefined || field.validation === "") continue;
    let pattern: RegExp;
    try {
      pattern = new RegExp(field.validation);
    } catch {
      continue;
    }
    if (!pattern.test(value)) problems[field.key] = CREDENTIAL_FORMAT_ERROR;
  }
  return problems;
}

/** Submit payload: every field trimmed (CLOUD-027). */
export function trimCredentialValues(
  fields: OpenhubCredentialField[],
  values: Record<string, string>
): Record<string, string> {
  const trimmed: Record<string, string> = {};
  for (const field of fields) trimmed[field.key] = (values[field.key] ?? "").trim();
  return trimmed;
}
