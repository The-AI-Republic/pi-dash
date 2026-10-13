// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { isRedirect } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";

import { Route } from "./route.js";

describe("gated layout guard", () => {
  const me = { is_password_autoset: false };

  function stubContext(values: Record<string, unknown>) {
    return {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            const value = values[options.queryKey[1] as string];
            if (value instanceof Error) throw value;
            if (value === undefined) throw new Error("no fixture");
            return value;
          },
        },
      },
      search: {},
      location: { href: "/acme/projects/p/issues" },
      params: {},
    } as never;
  }

  it("bounces signed-out visitors to the entry with a return path", async () => {
    const failed = new Error("signed out");
    const args = stubContext({ me: failed });
    try {
      await Route.options.beforeLoad?.(args);
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      const options = (error as unknown as { options: { to: unknown; search: unknown } }).options;
      expect(options.to).toBe("/");
      expect(options.search).toEqual({ next_path: "/acme/projects/p/issues" });
      return;
    }
    expect.unreachable("expected a redirect");
  });

  it("funnels unfinished members to onboarding", async () => {
    const args = stubContext({ me, profile: { is_onboarded: false } });
    try {
      await Route.options.beforeLoad?.(args);
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      expect((error as unknown as { options: { href: string } }).options.href).toBe("/onboarding");
      return;
    }
    expect.unreachable("expected a redirect");
  });

  it("renders for onboarded members", async () => {
    await expect(Route.options.beforeLoad?.(stubContext({ me, profile: { is_onboarded: true } }))).resolves.toEqual({});
  });
});
