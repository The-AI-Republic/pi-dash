// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CloudAdminEntry } from "./CloudAdminEntry.js";

const ADMIN_ME = {
  id: "22222222-2222-4222-8222-222222222222",
  avatar: "https://cdn.test/avatar.png",
  avatar_url: null,
  cover_image: null,
  date_joined: "2026-01-01T00:00:00Z",
  display_name: "Admin",
  email: "admin@example.com",
  first_name: "Admin",
  last_name: "User",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  user_timezone: "UTC",
  username: "admin",
  is_password_autoset: false,
};

const PROBE_USER = {
  id: "user-1",
  email: "user@example.com",
  display_name: "User",
  first_name: "User",
  last_name: "Person",
  avatar: "https://cdn.test/avatar.png",
};

interface ScriptedRoute {
  status: number;
  body: unknown;
}

/**
 * Queued responses per path: a queue of one replays on every hit, a
 * longer queue shifts per hit (for retry tests). Every hit is recorded.
 */
function stubCloudTransport(script: Record<string, ScriptedRoute[]>, seen: string[]): Transport {
  return (url) => {
    const path = new URL(url, "https://api.test").pathname;
    seen.push(path);
    const queue = script[path] ?? [{ status: 404, body: { detail: "not found" } }];
    const next = queue.length > 1 ? queue.shift() : queue[0];
    const route = next ?? { status: 500, body: { detail: "unscripted" } };
    return Promise.resolve({
      status: route.status,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(route.body)),
    });
  };
}

const realLocation = globalThis.window.location;

function stubLocation(pathname: string, search: string): { replace: ReturnType<typeof vi.fn> } {
  const replace = vi.fn();
  Object.defineProperty(globalThis.window, "location", {
    value: { replace, pathname, search },
    writable: true,
    configurable: true,
  });
  return { replace };
}

function renderEntry(
  script: Record<string, ScriptedRoute[]>,
  location: { pathname: string; search: string } = { pathname: "/god-mode/", search: "" }
): { replace: ReturnType<typeof vi.fn>; seen: string[] } {
  const seen: string[] = [];
  createAppClient("https://api.test", stubCloudTransport(script, seen));
  const { replace } = stubLocation(location.pathname, location.search);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CloudAdminEntry />
    </QueryClientProvider>
  );
  return { replace, seen };
}

afterEach(() => {
  Object.defineProperty(globalThis.window, "location", {
    value: realLocation,
    writable: true,
    configurable: true,
  });
  resetAppClient();
});

function expectNoForms(): void {
  expect(document.querySelector('input[type="password"]')).toBeNull();
  expect(screen.queryByLabelText("First name")).not.toBeInTheDocument();
}

describe("CloudAdminEntry", () => {
  it("shows only the loader while the check is unresolved", () => {
    createAppClient("https://api.test", (() => new Promise(() => {})) as Transport);
    const { replace } = stubLocation("/god-mode/", "");
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <CloudAdminEntry />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("admin-loader")).toBeInTheDocument();
    expectNoForms();
    expect(replace).not.toHaveBeenCalled();
  });

  it("holds the spinner for an authorized admin without probing or bouncing", async () => {
    const { replace, seen } = renderEntry({ "/api/instances/admins/me/": [{ status: 200, body: ADMIN_ME }] });
    await waitFor(() => expect(seen).toEqual(["/api/instances/admins/me/"]));
    expect(screen.getByTestId("admin-loader")).toBeInTheDocument();
    expectNoForms();
    expect(replace).not.toHaveBeenCalled();
    expect(screen.queryByTestId("admin-cloud-denied")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-cloud-error")).not.toBeInTheDocument();
  });

  it("bounces an anonymous read through OIDC without probing", async () => {
    const { replace, seen } = renderEntry({
      "/api/instances/admins/me/": [{ status: 401, body: { detail: "anonymous" } }],
    });
    await waitFor(() => expect(replace).toHaveBeenCalledTimes(1));
    expect(replace).toHaveBeenCalledWith("https://api.test/api/auth/sign-in/?return_to=%2Fgod-mode%2F");
    expect(seen).toEqual(["/api/instances/admins/me/"]);
    expect(screen.getByTestId("admin-loader")).toBeInTheDocument();
    expectNoForms();
  });

  it("preserves a deep link in the relative return_to", async () => {
    const { replace } = renderEntry(
      { "/api/instances/admins/me/": [{ status: 401, body: { detail: "anonymous" } }] },
      { pathname: "/god-mode/loop/3/", search: "?tab=targets" }
    );
    await waitFor(() => expect(replace).toHaveBeenCalledTimes(1));
    expect(replace).toHaveBeenCalledWith(
      "https://api.test/api/auth/sign-in/?return_to=%2Fgod-mode%2Floop%2F3%2F%3Ftab%3Dtargets"
    );
  });

  it("denies a forbidden read on an authenticated probe with no bounce", async () => {
    const { replace, seen } = renderEntry({
      "/api/instances/admins/me/": [{ status: 403, body: { detail: "forbidden" } }],
      "/api/auth/me/": [{ status: 200, body: PROBE_USER }],
    });
    await waitFor(() => expect(screen.getByTestId("admin-cloud-denied")).toBeInTheDocument());
    expect(seen).toEqual(["/api/instances/admins/me/", "/api/auth/me/"]);
    expect(replace).not.toHaveBeenCalled();
    expect(screen.getAllByRole("link")).toHaveLength(1);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expectNoForms();
  });

  it("bounces a forbidden read on an anonymous probe", async () => {
    const { replace, seen } = renderEntry({
      "/api/instances/admins/me/": [{ status: 403, body: { detail: "forbidden" } }],
      "/api/auth/me/": [{ status: 401, body: { detail: "anonymous" } }],
    });
    await waitFor(() => expect(replace).toHaveBeenCalledTimes(1));
    expect(replace).toHaveBeenCalledWith("https://api.test/api/auth/sign-in/?return_to=%2Fgod-mode%2F");
    expect(seen).toEqual(["/api/instances/admins/me/", "/api/auth/me/"]);
  });

  it("retries a forbidden read on an inconclusive probe, never bouncing or denying", async () => {
    const { replace, seen } = renderEntry({
      "/api/instances/admins/me/": [{ status: 403, body: { detail: "forbidden" } }],
      "/api/auth/me/": [{ status: 500, body: { detail: "blip" } }],
    });
    await waitFor(() => expect(screen.getByTestId("admin-cloud-error")).toBeInTheDocument());
    expect(seen).toEqual(["/api/instances/admins/me/", "/api/auth/me/"]);
    expect(replace).not.toHaveBeenCalled();
    expect(screen.queryByTestId("admin-cloud-denied")).not.toBeInTheDocument();
    expectNoForms();
  });

  it("retries a failed check without probing, bouncing, or denying", async () => {
    const { replace, seen } = renderEntry({
      "/api/instances/admins/me/": [{ status: 500, body: { detail: "blip" } }],
    });
    await waitFor(() => expect(screen.getByTestId("admin-cloud-error")).toBeInTheDocument());
    expect(seen).toEqual(["/api/instances/admins/me/"]);
    expect(replace).not.toHaveBeenCalled();
    expect(screen.queryByTestId("admin-cloud-denied")).not.toBeInTheDocument();
    expectNoForms();
  });

  it("retries a drifting probe instead of guessing on it", async () => {
    const { replace } = renderEntry({
      "/api/instances/admins/me/": [{ status: 403, body: { detail: "forbidden" } }],
      "/api/auth/me/": [{ status: 200, body: { bogus: true } }],
    });
    await waitFor(() => expect(screen.getByTestId("admin-cloud-error")).toBeInTheDocument());
    expect(replace).not.toHaveBeenCalled();
    expect(screen.queryByTestId("admin-cloud-denied")).not.toBeInTheDocument();
  });

  it("re-runs the check on retry and lands a recovered admin on the spinner", async () => {
    const { replace, seen } = renderEntry({
      "/api/instances/admins/me/": [
        { status: 500, body: { detail: "blip" } },
        { status: 200, body: ADMIN_ME },
      ],
    });
    await waitFor(() => expect(screen.getByTestId("admin-cloud-error")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(seen).toEqual(["/api/instances/admins/me/", "/api/instances/admins/me/"]));
    expect(screen.getByTestId("admin-loader")).toBeInTheDocument();
    expect(screen.queryByTestId("admin-cloud-error")).not.toBeInTheDocument();
    expect(replace).not.toHaveBeenCalled();
  });

  it("re-probes a fresh 403 on retry and denies on the recovered session", async () => {
    const { seen } = renderEntry({
      "/api/instances/admins/me/": [{ status: 403, body: { detail: "forbidden" } }],
      "/api/auth/me/": [
        { status: 500, body: { detail: "blip" } },
        { status: 200, body: PROBE_USER },
      ],
    });
    await waitFor(() => expect(screen.getByTestId("admin-cloud-error")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.getByTestId("admin-cloud-denied")).toBeInTheDocument());
    expect(seen.filter((path) => path === "/api/instances/admins/me/")).toHaveLength(2);
    expect(seen.filter((path) => path === "/api/auth/me/")).toHaveLength(2);
  });
});
