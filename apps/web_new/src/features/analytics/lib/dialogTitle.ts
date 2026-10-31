// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scoped dialog title (parity/specs/analytics.md §13, ANL-014). Pure: the
// three scope wordings the dialog can carry. A project with no narrowing
// names the project; a cycle or module scope appends "in {name}" once.
// The old title template left an empty cycle fragment behind, which read
// as a double space; one space is the intended wording (spec §13, §18).

/** What the dialog is scoped by, beyond its project. */
export interface DialogTitleScope {
  projectName: string;
  cycleName?: string | null | undefined;
  moduleName?: string | null | undefined;
}

/** `Analytics for {project}[ in {cycle|module}]`. */
export function scopedDialogTitle(scope: DialogTitleScope): string {
  const narrowed = scope.cycleName ?? scope.moduleName;
  return narrowed === null || narrowed === undefined
    ? `Analytics for ${scope.projectName}`
    : `Analytics for ${scope.projectName} in ${narrowed}`;
}
