// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Area-wide focus and keyboard conventions (AUTH-025, from
// parity/specs/auth.md). Every screen in this area follows these; sibling
// slices import this module instead of re-deciding:
// - each step autofocuses its primary input (authStepInputProps),
// - denial screens focus their primary action, including links, on mount
//   (useFocusOnMount: the autoFocus prop never fires on anchors),
// - steps submit through native forms so Enter submits with no bindings,
// - the email-step clear control stays pointer-only (pointerOnlyControlProps),
// - every other toggle, clear, dismiss, resend, and navigation control is a
//   keyboard-reachable button,
// - multi-row editors suppress Enter inside text fields so editing never
//   sends early (swallowEnterInTextField),
// - no screen requires custom key bindings.

import * as React from "react";

/** Props for an auth step's primary input: autofocused when it mounts. */
export function authStepInputProps(): { autoFocus: boolean } {
  return { autoFocus: true };
}

/**
 * Props for pointer-only controls (the email-step clear): reachable by
 * pointer but excluded from tab order. Every other auth control stays a
 * keyboard-reachable button.
 */
export function pointerOnlyControlProps(): { tabIndex: number } {
  return { tabIndex: -1 };
}

/**
 * A stable ref callback that focuses its element on mount. For primary
 * actions that are links: the autoFocus prop never fires on anchors, in
 * browsers or in jsdom, so denial screens focus through this instead.
 */
export function useFocusOnMount<T extends HTMLElement>(): (element: T | null) => void {
  return React.useCallback((element: T | null) => {
    element?.focus({ preventScroll: true });
  }, []);
}

interface KeyTarget {
  key: string;
  target: unknown;
}

/**
 * Whether an Enter keystroke inside a text field must be swallowed instead
 * of submitting the form: multi-row editors (the invite-members form)
 * never send early while the user is still editing rows. Callers
 * preventDefault when this answers true; Enter anywhere else (buttons,
 * textareas, single-field steps) keeps its native behavior.
 */
export function swallowEnterInTextField(event: KeyTarget): boolean {
  if (event.key !== "Enter") return false;
  if (typeof HTMLInputElement === "undefined") return false;
  if (!(event.target instanceof HTMLInputElement)) return false;
  return event.target.type === "text" || event.target.type === "email";
}
