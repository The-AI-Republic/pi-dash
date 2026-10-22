// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the inbox screen: its internals belong to the auth area.
// What this suite owns is the signed-in-only gate plus mounting.
vi.mock("../features/auth/index.js", () => ({
  InvitationInbox: () => <div data-testid="invitation-inbox" />,
}));

import { isRedirect } from "@tanstack/react-router";

import { Route } from "./invitations.js";

describe("invitations guard", () => {
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
      location: { href: "/invitations" },
      params: {},
    } as never;
  }

  it("renders the inbox for onboarded members", async () => {
    await expect(
      Route.options.beforeLoad?.(stubContext({ me: { is_password_autoset: false }, profile: { is_onboarded: true } }))
    ).resolves.toEqual({});
  });

  it("bounces visitors to the entry route with a return path", async () => {
    const failed = new ApiError({ status: 401, code: "http", message: "signed out" });
    try {
      await Route.options.beforeLoad?.(stubContext({ me: failed }));
      expect.unreachable("expected a redirect");
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      const options = (error as unknown as { options: { to: string; search: Record<string, string> } }).options;
      expect(options.to).toBe("/");
      expect(options.search).toEqual({ next_path: "/invitations" });
    }
  });

  it("funnels unfinished members to onboarding", async () => {
    try {
      await Route.options.beforeLoad?.(
        stubContext({ me: { is_password_autoset: true }, profile: { is_onboarded: false } })
      );
      expect.unreachable("expected a redirect");
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      expect((error as unknown as { options: { href: string } }).options.href).toBe("/onboarding");
    }
  });
});

describe("invitations route component", () => {
  it("mounts the invitation inbox", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("invitations route has no component");
    render(<Component />);
    expect(screen.getByTestId("invitation-inbox")).not.toBeNull();
  });
});
