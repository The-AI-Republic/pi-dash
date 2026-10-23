// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Space return addresses (parity/specs/space.md §1.3, SPC-006). One
// validator serves every space entry point — the landing `next_path`, the
// navbar sign-in link, the sign-out form, the vote/reaction/comment
// sign-in redirects, the auth forms' hidden field and every auth redirect
// the card follows. Values are subtree-relative: they start at the space
// root, carry no `/spaces` segment, keep their query, and the server
// re-attaches the mount on redirect. This deliberately does not import
// the workspace guard: same rules, space-owned function.

import { SPACE_BASE } from "./address.js";

/** Schemes that must never survive inside a return address, decoded. */
const ACTIVE_CONTENT = /(?:javascript|data|vbscript):/i;

/** Anything whitespace-shaped, decoded (covers tab/newline/space/unicode). */
const WHITESPACE = /\s/;

/** C0 controls and DEL, checked by code point (never a control regex). */
function hasControlCode(value: string): boolean {
  for (const char of value) {
    const code = char.codePointAt(0) as number;
    if (code <= 0x08 || (code >= 0x0e && code <= 0x1f) || code === 0x7f) return true;
  }
  return false;
}

/** How many decode rounds run before an over-encoded value fails closed. */
const MAX_DECODE_ROUNDS = 3;

function decodeFully(value: string): string | null {
  let decoded = value;
  for (let round = 0; round < MAX_DECODE_ROUNDS; round += 1) {
    let next: string;
    try {
      next = decodeURIComponent(decoded);
    } catch {
      return null;
    }
    if (next === decoded) return decoded;
    decoded = next;
  }
  return null;
}

function hasSingleLeadingSlash(value: string): boolean {
  return value.startsWith("/") && !value.startsWith("//");
}

/**
 * The shared SPC-006 check. Returns the value unchanged when every rule
 * passes, else null: a string starting with exactly one `/`, no
 * backslash, no decoded control/whitespace or active-content payload.
 * Same-origin needs no separate test: a value that passes those rules
 * cannot carry a scheme, an authority (`%2f%2f` decodes to a rejected
 * leading `//`) or an `@` authority trick (without a scheme `@` is path
 * data). Callers land dropped values on the safe default (the landing
 * document for the gate, the board for the sign-in links).
 */
export function validateSpaceReturnPath(value: unknown): string | null {
  if (typeof value !== "string" || value === "") return null;
  if (!hasSingleLeadingSlash(value)) return null;
  if (value.includes("\\")) return null;
  const decoded = decodeFully(value);
  if (decoded === null) return null;
  if (!hasSingleLeadingSlash(decoded)) return null;
  if (decoded.includes("\\")) return null;
  if (WHITESPACE.test(decoded) || hasControlCode(decoded)) return null;
  if (ACTIVE_CONTENT.test(decoded)) return null;
  return value;
}

export interface SpaceReturnTarget {
  /** Document path with the mount re-attached (e.g. `/spaces/issues/<a>/`). */
  to: string;
  /** The value's own query, decoded once. */
  search: Record<string, string>;
}

/**
 * Split a validated value into the document target a client navigation
 * needs. Fragments are dropped: they never survive the outer address
 * parse, so keeping them would only smuggle them into the target path.
 */
export function resolveSpaceReturnTarget(validated: string): SpaceReturnTarget {
  const hash = validated.indexOf("#");
  const withoutFragment = hash < 0 ? validated : validated.slice(0, hash);
  const query = withoutFragment.indexOf("?");
  const pathname = query < 0 ? withoutFragment : withoutFragment.slice(0, query);
  const search: Record<string, string> = {};
  if (query >= 0) {
    const params = new URLSearchParams(withoutFragment.slice(query + 1));
    params.forEach((paramValue, key) => {
      search[key] = paramValue;
    });
  }
  return { to: `${SPACE_BASE}${pathname}`, search };
}

/** True when the target is the landing document itself (never navigated to). */
export function isSpaceLandingTarget(target: SpaceReturnTarget): boolean {
  return target.to === SPACE_BASE || target.to === `${SPACE_BASE}/`;
}

export interface SpaceLandingSearch {
  nextPath?: string | undefined;
  email?: string | undefined;
  errorCode?: string | undefined;
}

function landingParam(search: Record<string, unknown>, key: string): string | undefined {
  const value = search[key];
  return typeof value === "string" && value !== "" ? value : undefined;
}

/**
 * Error codes off the address. The router decodes numeric spellings to
 * numbers, so integer values normalize to their string form; anything
 * else reads as absent.
 */
function landingErrorCode(search: Record<string, unknown>): string | undefined {
  const value = search["error_code"];
  if (typeof value === "string" && value !== "") return value;
  if (typeof value === "number" && Number.isInteger(value)) return String(value);
  return undefined;
}

/**
 * Lenient landing search parse for the route's `validateSearch`: present
 * non-empty strings pass through untouched (validation happens in the
 * gate, so a hostile `next_path` still renders the landing), everything
 * else reads as absent.
 */
export function parseSpaceLandingSearch(search: Record<string, unknown>): SpaceLandingSearch {
  const nextPath = landingParam(search, "next_path");
  const email = landingParam(search, "email");
  const errorCode = landingErrorCode(search);
  return {
    ...(nextPath === undefined ? {} : { nextPath }),
    ...(email === undefined ? {} : { email }),
    ...(errorCode === undefined ? {} : { errorCode }),
  };
}
