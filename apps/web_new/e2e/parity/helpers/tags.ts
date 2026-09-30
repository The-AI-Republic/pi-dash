// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Scenario tagging (NEWFRONT-19). Every parity scenario names the
// inventory rows it proves: row IDs ride in the test title in brackets and
// as lowercase Playwright tags, so `pnpm test:parity -- --grep @iss-007`
// runs one row and the parity report can join results back to inventory.

/** `[AUTH-001, ISS-007] sign in and list a project's issues`. */
export function specTitle(rowIds: string[], name: string): string {
  return `[${rowIds.join(", ")}] ${name}`;
}

/** `["@auth-001", "@iss-007"]` for Playwright `--grep`. */
export function specTags(rowIds: string[]): string[] {
  return rowIds.map((id) => `@${id.toLowerCase()}`);
}

/** Row IDs back out of a tagged title; empty when the title is untagged. */
export function rowIdsFromTitle(title: string): string[] {
  const match = /^\[([A-Za-z]+-\d+(?:\s*,\s*[A-Za-z]+-\d+)*)\]/.exec(title);
  if (match === null) return [];
  return (match[1] ?? "").split(",").map((part) => part.trim().toUpperCase());
}
