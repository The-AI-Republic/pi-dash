// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Address policy shared by every external-open path (DESK-015): the shell
// may only hand hosted web addresses to the operating system.

/** True only for absolute http(s) addresses. */
export function isWebAddress(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}
