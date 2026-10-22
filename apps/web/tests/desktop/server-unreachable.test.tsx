/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { cleanup, render, screen } from "@testing-library/react";
import { AxiosError } from "axios";
import { afterEach, describe, expect, it } from "vitest";
import { ServerUnreachableMessage, unreachableDesktopServer } from "@/components/instance/server-unreachable";

const networkError = () => new AxiosError("API request failed", AxiosError.ERR_NETWORK);
const serverError = () =>
  new AxiosError("Request failed with status code 502", AxiosError.ERR_BAD_RESPONSE, undefined, undefined, {
    status: 502,
  } as never);

function desktop() {
  Object.assign(window, {
    __PIDASH_NATIVE_HTTP__: "https://api.pidash.example.test",
    __TAURI__: { core: { invoke: () => Promise.resolve() } },
  });
}

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__PIDASH_NATIVE_HTTP__");
  Reflect.deleteProperty(window, "__TAURI__");
});

describe("unreachable desktop server", () => {
  it("names the server when the desktop request got no response", () => {
    desktop();
    expect(unreachableDesktopServer(networkError())).toBe("api.pidash.example.test");
  });

  it("keeps the maintenance message when the server answered", () => {
    desktop();
    expect(unreachableDesktopServer(serverError())).toBeUndefined();
    expect(unreachableDesktopServer(new Error("boom"))).toBeUndefined();
  });

  it("keeps the maintenance message in browsers", () => {
    expect(unreachableDesktopServer(networkError())).toBeUndefined();
  });

  it("tells the user which server could not be reached", () => {
    render(<ServerUnreachableMessage server="api.pidash.example.test" />);
    expect(screen.getByRole("heading").textContent).toBe("Pi Dash could not reach api.pidash.example.test");
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });
});
