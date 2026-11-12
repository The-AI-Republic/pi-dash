// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row factory shared by the States tab suites.
import type { State } from "@pidash/api-client";

export const TEST_PROJECT = "11111111-1111-4111-8111-111111111111";
export const TEST_WORKSPACE = "22222222-2222-4222-8222-222222222222";

export function testRowId(name: string): string {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  const hex = hash.toString(16).padStart(8, "0");
  return `00000000-0000-4000-8000-${hex}0000`;
}

export function state(overrides: Partial<State> & Pick<State, "name" | "group">): State {
  return {
    id: testRowId(overrides.name),
    project_id: TEST_PROJECT,
    workspace_id: TEST_WORKSPACE,
    color: "#000000",
    default: false,
    description: "",
    sequence: 1,
    ...overrides,
  };
}

export function stateList(
  names: { name: string; group: State["group"]; sequence?: number; default?: boolean }[]
): State[] {
  return names.map((entry, index) =>
    state({
      name: entry.name,
      group: entry.group,
      sequence: entry.sequence ?? index + 1,
      ...(entry.default === undefined ? {} : { default: entry.default }),
    })
  );
}
