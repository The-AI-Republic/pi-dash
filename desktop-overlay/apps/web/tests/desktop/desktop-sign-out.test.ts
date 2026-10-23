/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop sign-out has to end the session on the server without navigating the
 * window there: the app's navigation policy keeps every non-`/api/` server-host
 * navigation inside the bundle, so a form POST to `/auth/sign-out/` never
 * leaves the app and lands on the 404 route instead.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const isDesktop = vi.fn(() => true);
vi.mock("@/services/agent-runtime", () => ({ isDesktop: () => isDesktop() }));

const steps: string[] = [];
const invoke = vi.fn();
const requestCSRFToken = vi.fn();
const post = vi.fn();
const submit = vi.fn();
const replace = vi.fn();

beforeEach(() => {
  steps.length = 0;
  isDesktop.mockReturnValue(true);
  invoke.mockReset().mockImplementation(async (command: string) => {
    steps.push(command);
  });
  requestCSRFToken.mockReset().mockImplementation(async () => {
    steps.push("csrf");
    return { csrf_token: "csrf-1" };
  });
  // What the native transport answers once the server has signed the user
  // out: the view redirects to the hosted web app, which it refuses to follow.
  post.mockReset().mockImplementation(async () => {
    steps.push("post");
    throw Object.assign(new Error("API redirect is not allowed"), { code: "ERR_NETWORK" });
  });
  submit.mockReset().mockImplementation(() => steps.push("form-submit"));
  replace.mockReset().mockImplementation(() => steps.push("navigate"));
  (window as unknown as Record<string, unknown>).__TAURI__ = { core: { invoke } };
  vi.spyOn(HTMLFormElement.prototype, "submit").mockImplementation(submit);
  vi.spyOn(window.location, "replace").mockImplementation(replace);
});
afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI__;
  vi.restoreAllMocks();
});

describe("desktop sign-out", () => {
  it("signs out on the server before clearing the app's own session, without a form navigation", async () => {
    const { performSignOut } = await import("@/services/auth-signout");
    await performSignOut({ requestCSRFToken, post }, "https://pidash.test");

    expect(steps).toEqual(["csrf", "post", "desktop_clear_web_data", "navigate"]);
    expect(submit).not.toHaveBeenCalled();
    const [url, body] = post.mock.calls[0];
    expect(url).toBe("/auth/sign-out/");
    expect(String(body)).toBe("csrfmiddlewaretoken=csrf-1");
    // A route the bundle ships, not the server's `/auth/sign-out/`.
    expect(replace).toHaveBeenCalledWith("/");
  });

  it("stays signed in when the sign-out request does not reach the server", async () => {
    post.mockImplementation(async () => {
      steps.push("post");
      throw Object.assign(new Error("API request failed"), { code: "ERR_NETWORK" });
    });
    const { performSignOut } = await import("@/services/auth-signout");

    await expect(performSignOut({ requestCSRFToken, post }, "https://pidash.test")).rejects.toThrow(
      "API request failed"
    );
    expect(steps).toEqual(["csrf", "post"]);
  });

  it("stays signed in when the server rejects the sign-out", async () => {
    post.mockImplementation(async () => {
      steps.push("post");
      throw Object.assign(new Error("Request failed with status code 403"), { response: { status: 403 } });
    });
    const { performSignOut } = await import("@/services/auth-signout");

    await expect(performSignOut({ requestCSRFToken, post }, "https://pidash.test")).rejects.toThrow("403");
    expect(steps).toEqual(["csrf", "post"]);
  });

  it("stays signed in when the server answers with its CSRF failure page", async () => {
    // That page is rendered with a 200, so the request itself resolves.
    post.mockImplementation(async () => {
      steps.push("post");
      return { status: 200, data: "<!-- templates/csrf_failure.html -->" };
    });
    const { performSignOut } = await import("@/services/auth-signout");

    await expect(performSignOut({ requestCSRFToken, post }, "https://pidash.test")).rejects.toThrow(
      "Sign-out was not completed"
    );
    expect(steps).toEqual(["csrf", "post"]);
  });

  it("leaves the browser on the form POST", async () => {
    isDesktop.mockReturnValue(false);
    const { performSignOut } = await import("@/services/auth-signout");
    await performSignOut({ requestCSRFToken, post }, "https://pidash.test");

    expect(steps).toEqual(["csrf", "form-submit"]);
    const form = document.body.querySelector("form");
    expect(form?.getAttribute("action")).toBe("https://pidash.test/auth/sign-out/");
  });
});
