// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Optional-prop plumbing for kit components. The kit tsconfig enables
// exactOptionalPropertyTypes, and several Base UI parts type their props
// without an explicit undefined, so passing `open={undefined}` through
// fails typecheck. Spreading pickDefined instead keeps unset props absent.

export function pickDefined<T extends object>(values: T): { [K in keyof T]?: Exclude<T[K], undefined> } {
  const entries = Object.entries(values).filter(([, value]) => value !== undefined);
  return Object.fromEntries(entries) as { [K in keyof T]?: Exclude<T[K], undefined> };
}
