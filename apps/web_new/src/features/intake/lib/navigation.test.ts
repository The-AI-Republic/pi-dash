// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Previous/next movement (INT-027): neighbour order with wraparound and
// the typing/overlay guard.

import { describe, expect, it } from "vitest";

import { neighborIntakeId, suppressIntakeArrowKey } from "./navigation.js";

const ROWS = ["id-a", "id-b", "id-c"];

describe("neighborIntakeId", () => {
  it("steps to the neighbour in display order", () => {
    expect(neighborIntakeId(ROWS, "id-a", "next")).toBe("id-b");
    expect(neighborIntakeId(ROWS, "id-b", "prev")).toBe("id-a");
  });

  it("wraps at both ends", () => {
    expect(neighborIntakeId(ROWS, "id-c", "next")).toBe("id-a");
    expect(neighborIntakeId(ROWS, "id-a", "prev")).toBe("id-c");
  });

  it("stays on the only row", () => {
    expect(neighborIntakeId(["solo"], "solo", "next")).toBe("solo");
    expect(neighborIntakeId(["solo"], "solo", "prev")).toBe("solo");
  });

  it("steps onto the first or last row from an unknown selection", () => {
    expect(neighborIntakeId(ROWS, undefined, "next")).toBe("id-a");
    expect(neighborIntakeId(ROWS, undefined, "prev")).toBe("id-c");
    expect(neighborIntakeId(ROWS, "stale", "next")).toBe("id-a");
    expect(neighborIntakeId(ROWS, "stale", "prev")).toBe("id-c");
  });

  it("steps nowhere on an empty list", () => {
    expect(neighborIntakeId([], "id-a", "next")).toBeNull();
    expect(neighborIntakeId([], undefined, "prev")).toBeNull();
  });
});

describe("suppressIntakeArrowKey", () => {
  it("suppresses while an overlay is open whatever holds focus", () => {
    expect(suppressIntakeArrowKey(null, true)).toBe(true);
    expect(suppressIntakeArrowKey({ tagName: "BODY" } as unknown as EventTarget, true)).toBe(true);
  });

  it("suppresses on typing surfaces", () => {
    const textarea = { tagName: "TEXTAREA" } as unknown as EventTarget;
    expect(suppressIntakeArrowKey(textarea, false)).toBe(true);
    const editable = { tagName: "DIV", isContentEditable: true } as unknown as EventTarget;
    expect(suppressIntakeArrowKey(editable, false)).toBe(true);
  });

  it("lets arrows through on plain surfaces", () => {
    expect(suppressIntakeArrowKey(null, false)).toBe(false);
    expect(suppressIntakeArrowKey({ tagName: "BODY" } as unknown as EventTarget, false)).toBe(false);
    expect(suppressIntakeArrowKey({ tagName: "BUTTON" } as unknown as EventTarget, false)).toBe(false);
  });
});
