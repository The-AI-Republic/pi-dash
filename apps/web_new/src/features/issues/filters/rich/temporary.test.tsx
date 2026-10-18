// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import type { RichCondition } from "./expression.js";
import {
  FieldConfigRegistry,
  TEMPORARY_PREFIX,
  isTemporaryId,
  temporaryInstanceId,
  useTemporaryFieldConfigs,
} from "./temporary.js";

function condition(property: string, value: unknown = "x"): RichCondition {
  return { key: `k-${property}`, property, operator: "in", negated: false, value };
}

function Probe({
  conditions,
  registry,
}: {
  conditions: readonly RichCondition[];
  registry: FieldConfigRegistry;
}): React.ReactElement {
  useTemporaryFieldConfigs(conditions, registry);
  return <span data-testid="active">{registry.activeProperties().join(",")}</span>;
}

describe("temporary ids", () => {
  it("mints distinct ids under the TEMP- prefix", () => {
    const first = temporaryInstanceId();
    const second = temporaryInstanceId();
    expect(first).not.toBe(second);
    expect(first.startsWith(TEMPORARY_PREFIX)).toBe(true);
    expect(isTemporaryId(first)).toBe(true);
    expect(isTemporaryId("project:abc")).toBe(false);
  });
});

describe("FieldConfigRegistry", () => {
  it("registers, reads and disposes per property", () => {
    const registry = new FieldConfigRegistry();
    const rows = [condition("priority")];
    registry.register("priority", rows);
    expect(registry.read("priority")).toBe(rows);
    expect(registry.activeProperties()).toEqual(["priority"]);
    registry.dispose("priority");
    expect(registry.read("priority")).toBeUndefined();
    expect(registry.activeProperties()).toEqual([]);
  });

  it("replaces a field's entry on re-register", () => {
    const registry = new FieldConfigRegistry();
    registry.register("priority", [condition("priority", "low")]);
    const next = [condition("priority", "high")];
    registry.register("priority", next);
    expect(registry.read("priority")).toBe(next);
    expect(registry.activeProperties()).toEqual(["priority"]);
  });
});

describe("useTemporaryFieldConfigs", () => {
  it("registers each held field, refreshes edits and releases dropped fields", () => {
    const registry = new FieldConfigRegistry();
    const first = [condition("priority"), condition("")];
    const { rerender, unmount } = render(<Probe conditions={first} registry={registry} />);
    // A field with no property yet is not registered.
    expect(registry.activeProperties().sort()).toEqual(["priority"]);
    expect(registry.read("priority")).toBe(first);

    const edited = [condition("priority", "urgent"), condition("state_id")];
    rerender(<Probe conditions={edited} registry={registry} />);
    expect(registry.read("priority")).toBe(edited);
    expect(registry.activeProperties().sort()).toEqual(["priority", "state_id"]);

    const dropped = [condition("state_id")];
    rerender(<Probe conditions={dropped} registry={registry} />);
    expect(registry.read("priority")).toBeUndefined();
    expect(registry.activeProperties()).toEqual(["state_id"]);

    unmount();
    expect(registry.activeProperties()).toEqual([]);
  });

  it("shares the default registry across instances", () => {
    const first = [condition("priority")];
    const { unmount } = render(<Probe conditions={first} registry={new FieldConfigRegistry()} />);
    unmount();
    expect(() => render(<Probe conditions={[]} registry={new FieldConfigRegistry()} />)).not.toThrow();
  });

  it("leaves a registry clean when it unmounts with no fields", () => {
    const registry = new FieldConfigRegistry();
    registry.register("label", [condition("label")]);
    const { unmount } = render(<Probe conditions={[]} registry={registry} />);
    unmount();
    // It only removes what this instance held.
    expect(registry.activeProperties()).toEqual(["label"]);
  });
});
