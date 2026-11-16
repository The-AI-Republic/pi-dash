// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Label name rules (parity/specs/issues.md §19, ISS-226): the inline
// form requires a name capped at 255 characters, failing inline with
// the fixed messages below. The picker reuses the same rules for its
// inline creates.

/** Longest label name the form accepts. */
export const LABEL_NAME_MAX_LENGTH = 255;

/** Inline error for a blank label name. */
export const LABEL_NAME_REQUIRED_MESSAGE = "Label title is required";

/** Inline error for an over-long label name. */
export const LABEL_NAME_TOO_LONG_MESSAGE = "Label name should not exceed 255 characters";

/** Validate a label name, returning the inline error or null when valid. */
export function validateLabelName(name: string): string | null {
  if (name.trim() === "") return LABEL_NAME_REQUIRED_MESSAGE;
  if (name.length > LABEL_NAME_MAX_LENGTH) return LABEL_NAME_TOO_LONG_MESSAGE;
  return null;
}
