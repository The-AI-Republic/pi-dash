// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Install-callback handling (parity/specs/profile.md §11, PROF-025). The
// code-hosting host sends the member back to the integrations address with
// a result marker: success names the workspace, failure names a code. The
// marker becomes a single notice, the named workspace becomes the
// selection, and the marker keys disappear from the address while every
// unrelated parameter stays exactly where it was.

import { CALLBACK_FAILED_FALLBACK } from "./copy.js";

export interface InstallCallback {
  ok: boolean;
  /** The workspace the host reported connecting, when it named one. */
  workspaceSlug: string | null;
  /** The notice detail for a failure, already worded for the member. */
  message: string;
}

/** Failure codes the host can send, with the wording the member sees. */
export const INSTALL_ERROR_MESSAGES: Readonly<Record<string, string>> = {
  actor_mismatch: "This install session was started by a different Pi Dash user.",
  github_verification_failed: "Pi Dash could not verify the GitHub installation.",
  login_required: "Sign in to Pi Dash and start the GitHub App install again.",
  installation_not_visible_to_user: "Your GitHub account cannot access that installation.",
  missing_installation_id: "GitHub did not return an installation id.",
  missing_oauth_code: "GitHub did not return a setup authorization code.",
  workspace_admin_required: "You must be a workspace admin to connect GitHub.",
};

/** Read the callback off a query string, or null when there is none. */
export function readInstallCallback(query: string): InstallCallback | null {
  const params = new URLSearchParams(query);
  const result = params.get("github_app");
  if (result === null || result === "") return null;
  if (result === "connected") {
    const slug = params.get("workspace_slug");
    return { ok: true, workspaceSlug: slug === null || slug === "" ? null : slug, message: "" };
  }
  const code = params.get("error") ?? result;
  return {
    ok: false,
    workspaceSlug: null,
    message: INSTALL_ERROR_MESSAGES[code] ?? CALLBACK_FAILED_FALLBACK,
  };
}

/**
 * The query string without the callback keys. Everything else keeps its
 * order and its value; an empty result means "no query at all".
 */
export function scrubInstallCallback(query: string): string {
  const params = new URLSearchParams(query);
  for (const key of ["github_app", "workspace_slug", "error"]) params.delete(key);
  return params.toString();
}

/** The address a callback page should end on, hash and path preserved. */
export function scrubbedAddress(pathname: string, search: string, hash = ""): string {
  const remaining = scrubInstallCallback(search);
  return `${pathname}${remaining === "" ? "" : `?${remaining}`}${hash}`;
}
