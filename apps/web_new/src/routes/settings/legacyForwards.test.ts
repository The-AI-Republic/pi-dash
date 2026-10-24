// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy settings addresses (parity/specs/profile.md §5, PROF-026):
// every family's route forwards to its tab address, keeping the query.
import { isNotFound, isRedirect } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";

import { Route as Account } from "../$ws/_gated/settings/account/$.js";
import { Route as ApiTokens } from "../$ws/_gated/settings/api-tokens.js";
import { Route as Profile } from "../profile/$.js";
import { Route as SettingsIndex } from "./index.js";

interface Redirected {
  options: { href: string };
}

async function thrown(run: () => unknown): Promise<unknown> {
  try {
    await run();
  } catch (error) {
    return error;
  }
  return null;
}

async function forwardOf(run: () => unknown): Promise<string> {
  const error = await thrown(run);
  expect(isRedirect(error)).toBe(true);
  return (error as Redirected).options.href;
}

describe("legacy settings forwards", () => {
  it("takes the bare settings address to the general tab", async () => {
    expect(
      await forwardOf(() =>
        SettingsIndex.options.beforeLoad?.({ location: { pathname: "/settings/", searchStr: "" } } as never)
      )
    ).toBe("/settings/profile/general");
  });

  it("takes bare profile and account addresses to the general tab", async () => {
    expect(
      await forwardOf(() =>
        Profile.options.beforeLoad?.({ location: { pathname: "/profile/", searchStr: "?a=1" } } as never)
      )
    ).toBe("/settings/profile/general?a=1");
    expect(
      await forwardOf(() =>
        Account.options.beforeLoad?.({ location: { pathname: "/acme/settings/account/", searchStr: "" } } as never)
      )
    ).toBe("/settings/profile/general");
  });

  it("maps legacy tab segments to the matching tab", async () => {
    expect(
      await forwardOf(() =>
        Profile.options.beforeLoad?.({ location: { pathname: "/profile/security/", searchStr: "" } } as never)
      )
    ).toBe("/settings/profile/security");
    expect(
      await forwardOf(() =>
        Account.options.beforeLoad?.({
          location: { pathname: "/acme/settings/account/notifications/", searchStr: "?x=1" },
        } as never)
      )
    ).toBe("/settings/profile/notifications?x=1");
  });

  it("defaults an unknown legacy tab to the general tab", async () => {
    expect(
      await forwardOf(() =>
        Account.options.beforeLoad?.({
          location: { pathname: "/acme/settings/account/bogus/", searchStr: "" },
        } as never)
      )
    ).toBe("/settings/profile/general");
  });

  it("answers a deeper address with the not-found page", async () => {
    const error = await thrown(() =>
      Profile.options.beforeLoad?.({
        location: { pathname: "/profile/security/extra", searchStr: "" },
      } as never)
    );
    expect(isNotFound(error)).toBe(true);
  });

  it("maps the workspace tokens address to the tokens tab with its query", async () => {
    expect(
      await forwardOf(() =>
        ApiTokens.options.beforeLoad?.({
          location: { pathname: "/acme/settings/api-tokens/", searchStr: "?page=2" },
        } as never)
      )
    ).toBe("/settings/profile/api-tokens/?page=2");
  });
});
