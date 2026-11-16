// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Area context (NEWFRONT-540). Child slices read one resolved scope: the
// base, a child-path builder and the access decision.
import { renderHook } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { buildRunnersAreaValue, RunnersAreaProvider, useRunnersArea } from "./area.js";

describe("buildRunnersAreaValue", () => {
  it("exposes the workspace base and access", () => {
    const value = buildRunnersAreaValue({ workspaceSlug: "acme" }, "ws-id", "allowed");
    expect(value.scope).toEqual({ workspaceSlug: "acme" });
    expect(value.workspaceId).toBe("ws-id");
    expect(value.access).toBe("allowed");
    expect(value.allowed).toBe(true);
    expect(value.base).toBe("/acme/runners");
    expect(value.path()).toBe("/acme/runners");
    expect(value.path("runs")).toBe("/acme/runners/runs");
  });

  it("rewrites the base for a project scope and marks every non-allowed access", () => {
    const value = buildRunnersAreaValue({ workspaceSlug: "acme", projectId: "p-1" }, "ws-id", "denied");
    expect(value.base).toBe("/acme/projects/p-1/runners");
    expect(value.path("chat/r-1")).toBe("/acme/projects/p-1/runners/chat/r-1");
    expect(value.allowed).toBe(false);
    expect(buildRunnersAreaValue({ workspaceSlug: "acme" }, "ws-id", "loading").allowed).toBe(false);
  });
});

describe("useRunnersArea", () => {
  it("returns the provided value", () => {
    const value = buildRunnersAreaValue({ workspaceSlug: "acme" }, "ws-id", "allowed");
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <RunnersAreaProvider value={value}>{children}</RunnersAreaProvider>
    );
    const { result } = renderHook(() => useRunnersArea(), { wrapper });
    expect(result.current).toBe(value);
  });

  it("throws outside the area", () => {
    expect(() => renderHook(() => useRunnersArea())).toThrow(/inside a runners route/);
  });
});
