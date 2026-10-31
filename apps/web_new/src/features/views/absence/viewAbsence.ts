// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views absence auditor (VIEW-047, from parity/specs/views.md §13):
// a DOM-level proof that the views surface offers no drag reordering
// and no export, import or print action. Suites render a screen or a
// dialog and audit it to zeros; the node scan beside this module pins
// the same absences in every source file, including screens no render
// test mounts. DOM-only: no framework imports, so any suite can use it.

/**
 * Transfer words with word boundaries, mirroring the parity driver's
 * scan: bare "import" would also match "important".
 */
const TRANSFER_NAME_PATTERN = /\bexport\b|\bimport\b|\bprint\b/i;

/**
 * The name a control announces: its explicit label first, else its
 * text. Empty labels fall through to text, matching name computation.
 */
function controlName(element: Element): string {
  return element.getAttribute("aria-label") || element.textContent || "";
}

/** The transfer surface under a root: every count must stay zero. */
export interface ViewAbsenceAudit {
  /** Elements opted into native dragging. */
  draggable: number;
  /** File choosers. */
  fileInputs: number;
  /** Buttons or links offering an export, import or print action. */
  transferControls: number;
}

/**
 * Count the transfer surface under `root` (VIEW-047): native drag
 * sources, file choosers, and export/import/print controls. Selectors
 * match the parity driver's absence facts one for one.
 */
export function auditViewAbsence(root: ParentNode): ViewAbsenceAudit {
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
