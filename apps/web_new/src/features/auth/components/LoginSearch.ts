// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Search the /login address reads (AUTH-044, CLOUD-002): the entry card's
// return path and prefill fields, plus the desktop failure landing. Lives
// with the auth cards the login route renders, so the eager route module
// carries only the return-path parse its guard needs.
import { asString, parseReturnPath } from "../../../core/session/index.js";

export interface LoginSearch {
  next?: string | undefined;
  unsafeNext?: string | undefined;
  email?: string | undefined;
  error_code?: string | undefined;
  invitation_id?: string | undefined;
  slug?: string | undefined;
  /** Sanitized return target (the desktop landing reads this spelling). */
  nextPath?: string | undefined;
  error?: string | undefined;
  errorDescription?: string | undefined;
}

/** Lenient parser: invalid params fall back to absent instead of throwing. */
export function parseLoginSearch(search: Record<string, unknown>): LoginSearch {
  const { next, unsafeNext } = parseReturnPath(search);
  const email = asString(search["email"]);
  const errorCode = asString(search["error_code"]);
  const invitationId = asString(search["invitation_id"]);
  const slug = asString(search["slug"]);
  const error = asString(search["error"]);
  const errorDescription = asString(search["error_description"]);
  return {
    ...(next === undefined ? {} : { next, nextPath: next }),
    ...(unsafeNext === undefined ? {} : { unsafeNext }),
    ...(email === undefined ? {} : { email }),
    ...(errorCode === undefined ? {} : { error_code: errorCode }),
    ...(invitationId === undefined ? {} : { invitation_id: invitationId }),
    ...(slug === undefined ? {} : { slug }),
    ...(error === undefined ? {} : { error }),
    ...(errorDescription === undefined ? {} : { errorDescription }),
  };
}
