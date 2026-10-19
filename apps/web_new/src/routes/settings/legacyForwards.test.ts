// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy settings addresses (parity/specs/profile.md §5, PROF-026):
// every family's route forwards to its tab address, keeping the query.
import { isRedirect } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";

import { Route as AccountIndex } from "../$ws/_gated/settings/account/index.js";
import { Route as AccountTab } from "../$ws/_gated/settings/account/$tab.js";
import { Route as ApiTokens } from "../$ws/_gated/settings/api-tokens.js";
import { Route as ProfileIndex } from "../profile/index.js";
import { Route as ProfileTab } from "../profile/$tab.js";
import { Route as SettingsIndex } from "./index.js";

interface Redirected {
  options: { href: string };
}

function thrown(run: () => unknown): unknown {
  try {
    run();
  } catch (error) {
    return error;
  }
  return null;
}

function forwardOf(run: () => unknown): string {
  const error = thrown(run);
  expect(isRedirect(error)).toBe(true);
  return (error as Redirected).options.href;
}

describe("legacy settings forwards", () => {
  it("takes the bare settings address to the general tab", () => {
    expect(
      forwardOf(() =>
        SettingsIndex.options.beforeLoad?.({ location: { pathname: "/settings/", searchStr: "" } } as never)
      )
    ).toBe("/settings/profile/general/");
  });

  it("takes bare profile and account addresses to the general tab", () => {
    expect(
      forwardOf(() =>
        ProfileIndex.options.beforeLoad?.({ location: { pathname: "/profile/", searchStr: "?a=1" } } as never)
      )
    ).toBe("/settings/profile/general/?a=1");
    expect(
      forwardOf(() =>
        AccountIndex.options.beforeLoad?.({ location: { pathname: "/acme/settings/account/", searchStr: "" } } as never)
      )
    ).toBe("/settings/profile/general/");
  });

  it("maps legacy tab segments to the matching tab", () => {
    expect(
      forwardOf(() =>
        ProfileTab.options.beforeLoad?.({ location: { pathname: "/profile/security/", searchStr: "" } } as never)
      )
    ).toBe("/settings/profile/security/");
    expect(
      forwardOf(() =>
        AccountTab.options.beforeLoad?.({
          location: { pathname: "/acme/settings/account/notifications/", searchStr: "?x=1" },
        } as never)
      )
    ).toBe("/settings/profile/notifications/?x=1");
  });

  it("defaults an unknown legacy tab to the general tab", () => {
    expect(
      forwardOf(() =>
        AccountTab.options.beforeLoad?.({
          location: { pathname: "/acme/settings/account/bogus/", searchStr: "" },
        } as never)
      )
    ).toBe("/settings/profile/general/");
  });

  it("maps the workspace tokens address to the tokens tab with its query", () => {
    expect(
      forwardOf(() =>
        ApiTokens.options.beforeLoad?.({
          location: { pathname: "/acme/settings/api-tokens/", searchStr: "?page=2" },
        } as never)
      )
    ).toBe("/settings/profile/api-tokens/?page=2");
  });
});
