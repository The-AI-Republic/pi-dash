// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare account-settings address (PROF-009/026; main#670): the collection
// forwards to the general tab of the workspace in the address, keeping the
// query string and the fragment.
import { isRedirect } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";

import { Route } from "./index.js";

function forward(ws: string, searchStr = "", hash = ""): unknown | undefined {
  try {
    Route.options.beforeLoad?.({
      params: { ws },
      location: { pathname: `/${ws}/settings/account/`, searchStr, hash },
    } as never);
  } catch (error) {
    return error;
  }
  return undefined;
}

describe("bare account settings address", () => {
  it("forwards to the general tab of its workspace", () => {
    const thrown = forward("acme");
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/acme/settings/account/general/");
  });

  it("keeps the query string", () => {
    const thrown = forward("globex", "?keep=1");
    expect((thrown as { options: { href: string } }).options.href).toBe("/globex/settings/account/general/?keep=1");
  });

  it("keeps the fragment", () => {
    const thrown = forward("globex", "", "#section");
    expect((thrown as { options: { href: string } }).options.href).toBe("/globex/settings/account/general/#section");
  });
});
