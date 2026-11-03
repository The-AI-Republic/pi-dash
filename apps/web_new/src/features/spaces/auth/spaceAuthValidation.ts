// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Client-side gates for the public spaces auth card (parity/specs/space.md
// SPC-058, SPC-061). The address check keeps obvious typos on the card; the
// password rule mirrors the server's observable acceptance (eight characters
// plus one of each class) so weak secrets never reach the form post. The
// server stays authoritative in both cases: it re-validates the address on
// the email check and the secret on sign-up, and its refusals arrive as
// error codes the card's catalog turns into banners.
const MAX_ADDRESS_LENGTH = 254;
const MAX_LOCAL_PART_LENGTH = 64;
const MIN_TOP_LEVEL_LENGTH = 2;

/**
 * Whether the value has a plausible address shape: one `@`, a non-empty
 * local part without whitespace, and a dotted domain whose top level is
 * letters. Trimmed before reading; the empty value never passes.
 */
export function isPlausibleSpaceEmail(value: string): boolean {
  const trimmed = value.trim();
  if (trimmed === "" || trimmed.length > MAX_ADDRESS_LENGTH || /\s/.test(trimmed)) return false;
  const at = trimmed.indexOf("@");
  if (at <= 0 || at !== trimmed.lastIndexOf("@") || at === trimmed.length - 1) return false;
  const local = trimmed.slice(0, at);
  const domain = trimmed.slice(at + 1);
  if (local.length > MAX_LOCAL_PART_LENGTH) return false;
  if (local.startsWith(".") || local.endsWith(".") || local.includes("..")) return false;
  if (!/^[A-Za-z0-9.-]+$/.test(domain) || domain.includes("..")) return false;
  const lastDot = domain.lastIndexOf(".");
  if (lastDot <= 0 || lastDot === domain.length - 1) return false;
  const topLevel = domain.slice(lastDot + 1);
  return topLevel.length >= MIN_TOP_LEVEL_LENGTH && /^[A-Za-z]+$/.test(topLevel);
}

/** Minimum password length the sign-up meter enforces. */
export const SPACE_PASSWORD_MIN_LENGTH = 8;

export type SpacePasswordCriterionId = "length" | "upper" | "lower" | "digit" | "symbol";

export interface SpacePasswordCriterion {
  id: SpacePasswordCriterionId;
  label: string;
  met: boolean;
}

function isUpperLetter(code: number): boolean {
  return code >= 65 && code <= 90;
}

function isLowerLetter(code: number): boolean {
  return code >= 97 && code <= 122;
}

function isDigit(code: number): boolean {
  return code >= 48 && code <= 57;
}

/**
 * Whether the code unit counts as a symbol: ASCII punctuation except the
 * space, backslash, backtick and tilde, which never satisfy the rule.
 */
function isAcceptedSymbol(code: number): boolean {
  if (code >= 33 && code <= 47) return true;
  if (code >= 58 && code <= 64) return true;
  if (code === 91 || code === 93 || code === 94 || code === 95) return true;
  return code >= 123 && code <= 125;
}

/** Per-rule breakdown for the sign-up strength meter. */
export function spacePasswordCriteria(password: string): SpacePasswordCriterion[] {
  let upper = false;
  let lower = false;
  let digit = false;
  let symbol = false;
  for (const char of password) {
    const code = char.charCodeAt(0);
    if (isUpperLetter(code)) upper = true;
    if (isLowerLetter(code)) lower = true;
    if (isDigit(code)) digit = true;
    if (isAcceptedSymbol(code)) symbol = true;
  }
  return [
    { id: "length", label: "8+ characters", met: password.length >= SPACE_PASSWORD_MIN_LENGTH },
    { id: "upper", label: "Uppercase letter", met: upper },
    { id: "lower", label: "Lowercase letter", met: lower },
    { id: "digit", label: "Number", met: digit },
    { id: "symbol", label: "Symbol", met: symbol },
  ];
}

/** True once every strength rule passes; the empty secret never passes. */
export function isStrongSpacePassword(password: string): boolean {
  return spacePasswordCriteria(password).every((criterion) => criterion.met);
}
