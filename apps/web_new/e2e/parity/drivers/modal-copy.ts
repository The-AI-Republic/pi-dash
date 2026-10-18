// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Confirmation-copy extraction for the parity driver (NEWFRONT-540,
// RUN-010). The kit renders the dialog description as the element right
// after the heading, inside the title block; reading from the heading keeps
// the driver independent of the heading level and wrapper nesting.

/** The description text that follows `element` (used as `evaluate` body). */
export function descriptionAfterHeading(element: Element): string {
  const sibling = element.nextElementSibling;
  if (!sibling) return "";
  const text = (sibling as HTMLElement).innerText ?? sibling.textContent ?? "";
  return text.trim();
}
