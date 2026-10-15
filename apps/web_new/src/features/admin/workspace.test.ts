// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { InstanceConfigurationList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  DISABLE_WORKSPACE_CREATION_KEY,
  encodeWorkspaceCreationRestricted,
  isWorkspaceCreationRestricted,
  mergeConfigurationRows,
  selectConfigurationValue,
} from "./workspace.js";

function row(key: string, value: string | null): InstanceConfigurationList[number] {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "test",
    is_encrypted: false,
    source: "db",
    is_managed: false,
  };
}

describe("workspace creation restriction flag", () => {
  it("names the stored configuration key", () => {
    expect(DISABLE_WORKSPACE_CREATION_KEY).toBe("DISABLE_WORKSPACE_CREATION");
  });

  it("reads numeric nonzero as restricted (§0.6)", () => {
    expect(isWorkspaceCreationRestricted("1")).toBe(true);
    expect(isWorkspaceCreationRestricted("2")).toBe(true);
    expect(isWorkspaceCreationRestricted("-1")).toBe(true);
  });

  it("reads zero, missing, and unparseable values as open", () => {
    expect(isWorkspaceCreationRestricted("0")).toBe(false);
    expect(isWorkspaceCreationRestricted(null)).toBe(false);
    expect(isWorkspaceCreationRestricted(undefined)).toBe(false);
    expect(isWorkspaceCreationRestricted("")).toBe(false);
    expect(isWorkspaceCreationRestricted("off")).toBe(false);
  });

  it("encodes the restriction back to the stored flag", () => {
    expect(encodeWorkspaceCreationRestricted(true)).toBe("1");
    expect(encodeWorkspaceCreationRestricted(false)).toBe("0");
  });
});

describe("mergeConfigurationRows", () => {
  it("seeds a missing cache from the answer alone", () => {
    const saved = [row("A", "1")];
    expect(mergeConfigurationRows(undefined, saved)).toEqual(saved);
  });

  it("replaces touched keys and keeps untouched ones", () => {
    const cached = [row("A", "0"), row("B", "kept")];
    const merged = mergeConfigurationRows(cached, [row("A", "1")]);
    expect(merged).toEqual([row("A", "1"), row("B", "kept")]);
  });

  it("appends answered rows the cache has not seen", () => {
    const merged = mergeConfigurationRows([row("A", "0")], [row("B", "new")]);
    expect(merged).toEqual([row("A", "0"), row("B", "new")]);
  });
});

describe("selectConfigurationValue", () => {
  it("reads the raw value for a known key", () => {
    expect(selectConfigurationValue([row("A", "1")], "A")).toBe("1");
    expect(selectConfigurationValue([row("A", null)], "A")).toBe(null);
  });

  it("answers undefined for unknown keys and a missing map", () => {
    expect(selectConfigurationValue([row("A", "1")], "B")).toBe(undefined);
    expect(selectConfigurationValue(undefined, "A")).toBe(undefined);
  });
});
