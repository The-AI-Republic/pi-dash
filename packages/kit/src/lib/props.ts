// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Optional-prop plumbing for kit components. The kit tsconfig enables
// exactOptionalPropertyTypes, and several Base UI parts type their props
// without an explicit undefined, so passing `open={undefined}` through
// fails typecheck. Spreading pickDefined instead keeps unset props absent.

export function pickDefined<T extends object>(values: T): { [K in keyof T]?: Exclude<T[K], undefined> } {
  const entries = Object.entries(values).filter(([, value]) => value !== undefined);
  return Object.fromEntries(entries) as { [K in keyof T]?: Exclude<T[K], undefined> };
}
