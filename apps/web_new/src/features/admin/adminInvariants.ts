// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cross-page invariant auditors (ADM-069–071, from
// parity/specs/admin.md §9): DOM-level proofs of the console's
// load-bearing absences. Page slices call these in their own suites,
// so every dashboard page inherits the pins; the area gate proves the
// same rows across pages in shell.spec.ts. DOM-only: no framework
// imports, so any suite can use them.

/**
 * Transfer words with word boundaries, mirroring the parity driver's
 * scan: bare "import" would also match "important".
 */
const TRANSFER_NAME_PATTERN = /\bimport\b|\bexport\b/i;

/** Secret-field affordance names, always a show/hide verb first. */
const TOGGLE_NAME_PATTERN = /^(show|hide)\s/i;

/**
 * The name a control announces: its explicit label first, else its
 * text. Empty labels fall through to text, matching name computation.
 */
function controlName(element: Element): string {
  return element.getAttribute("aria-label") || element.textContent || "";
}

/** The transfer surface under a root: every count must stay zero. */
export interface AdminAbsenceAudit {
  /** Elements opted into native dragging. */
  draggable: number;
  /** File choosers. */
  fileInputs: number;
  /** Buttons or links offering an import/export action. */
  transferControls: number;
}

/**
 * Count the transfer surface under `root` (ADM-070): native drag
 * sources, file choosers, and import/export controls. Selectors match
 * the parity driver's absence facts one for one.
 */
export function auditAdminAbsence(root: ParentNode): AdminAbsenceAudit {
  let transferControls = 0;
  for (const control of root.querySelectorAll("button, a")) {
    if (TRANSFER_NAME_PATTERN.test(controlName(control))) {
      transferControls += 1;
    }
  }
  return {
    draggable: root.querySelectorAll('[draggable="true"]').length,
    fileInputs: root.querySelectorAll('input[type="file"]').length,
    transferControls,
  };
}

/** The tab-order footprint of show/hide toggles under a root. */
export interface AdminToggleAudit {
  /** Show/hide toggles found. */
  toggles: number;
  /** How many of them sit in tab order; must stay zero. */
  tabbableToggles: number;
}

/**
 * Count show/hide toggles under `root` and how many are tabbable
 * (ADM-071). Secret-field toggles are pointer conveniences only: each
 * carries tabindex -1, so keyboard users tab straight past them.
 */
export function auditAdminToggles(root: ParentNode): AdminToggleAudit {
  let toggles = 0;
  let tabbableToggles = 0;
  for (const button of root.querySelectorAll("button")) {
    if (!TOGGLE_NAME_PATTERN.test(controlName(button))) {
      continue;
    }
    toggles += 1;
    if ((button as HTMLElement).tabIndex >= 0) {
      tabbableToggles += 1;
    }
  }
  return { toggles, tabbableToggles };
}
