// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The public-board subtree is web-only (desktop spec §11, D13): web builds
// pass the gate, desktop builds never mount a board. Only the mount is
// exercised here; the board layouts have their own suites.
import { isNotFound } from "@tanstack/react-router";
import { afterEach, describe, expect, it, vi } from "vitest";

const platformHarness = vi.hoisted(() => ({ kind: "web" }));
const runtime = vi.hoisted(() => ({ queryClient: null as unknown }));

vi.mock("../../core/platform/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../core/platform/index.js")>();
  return {
    ...actual,
    get platform() {
      return platformHarness;
    },
  };
});

vi.mock("../../features/spaces/api/client.js", () => ({ spaceRuntime: runtime }));

import { Route } from "./route.js";

function gate(): (args: unknown) => unknown {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("spaces layout route has no beforeLoad");
  return beforeLoad as (args: unknown) => unknown;
}

describe("public boards web-only gate", () => {
  afterEach(() => {
    platformHarness.kind = "web";
  });

  it("passes web builds through to the boards", () => {
    expect(gate()({} as never)).toEqual({});
  });

  it("throws not-found on desktop builds", () => {
    platformHarness.kind = "desktop";
    let thrown: unknown;
    try {
      gate()({} as never);
    } catch (error) {
      thrown = error;
    }
    expect(isNotFound(thrown)).toBe(true);
  });
});
