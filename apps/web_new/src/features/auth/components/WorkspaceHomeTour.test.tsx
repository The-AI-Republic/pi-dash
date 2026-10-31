// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { setSessionClient } from "../../../core/session/queries.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import { TOUR_SKIP_LABEL } from "./ProductTour.js";
import { WorkspaceHomeTour } from "./WorkspaceHomeTour.js";

function stubTransport(
  handler: (url: string, init: TransportInit) => { status: number; body: string; url: string }
): Transport {
  return async (url, init) => {
    const response = handler(url, init);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

function renderSeam(options: {
  profile?: { is_tour_completed: boolean } | "loading";
  me?: { first_name: string } | "missing";
  tourStatus?: number;
  onCreateProject?: () => void;
}): { posted: unknown[]; queryClient: QueryClient } {
  const posted: unknown[] = [];
  createAppClient(
    "https://api.test",
    stubTransport((url, init) => {
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/users/me/tour-completed/")) {
        posted.push(JSON.parse(init.body as string));
        if ((options.tourStatus ?? 200) !== 200) return { status: 500, body: JSON.stringify({ detail: "boom" }), url };
        return { status: 200, body: JSON.stringify({ message: "Tour completed" }), url };
      }
      return { status: 200, body: "{}", url };
    })
  );
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.profile !== undefined && options.profile !== "loading") {
    queryClient.setQueryData(["session", "profile"], options.profile);
  }
  if (options.me !== undefined && options.me !== "missing") {
    queryClient.setQueryData(["session", "me"], options.me);
  }
  render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceHomeTour {...(options.onCreateProject ? { onCreateProject: options.onCreateProject } : {})} />
    </QueryClientProvider>
  );
  return { posted, queryClient };
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
});

describe("WorkspaceHomeTour", () => {
  it("renders nothing while the profile is still loading", () => {
    renderSeam({ profile: "loading", me: { first_name: "Ada" } });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("renders nothing once the tour flag is set", () => {
    renderSeam({ profile: { is_tour_completed: true }, me: { first_name: "Ada" } });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("greets by first name and dismisses through the flag write", async () => {
    const user = userEvent.setup();
    const { posted, queryClient } = renderSeam({ profile: { is_tour_completed: false }, me: { first_name: "Ada" } });
    expect(screen.getByRole("heading", { name: "Welcome to Pi Dash, Ada" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: TOUR_SKIP_LABEL }));
    await waitFor(() => expect(posted).toEqual([{ is_tour_completed: true }]));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect((queryClient.getQueryData(["session", "profile"]) as { is_tour_completed: boolean }).is_tour_completed).toBe(
      true
    );
  });

  it("greets plainly when the user row has not loaded", () => {
    renderSeam({ profile: { is_tour_completed: false }, me: "missing" });
    expect(screen.getByRole("heading", { name: "Welcome to Pi Dash" })).toBeInTheDocument();
  });

  it("keeps the overlay up when the flag write fails", async () => {
    const user = userEvent.setup();
    renderSeam({ profile: { is_tour_completed: false }, me: { first_name: "Ada" }, tourStatus: 500 });
    await user.click(screen.getByRole("button", { name: TOUR_SKIP_LABEL }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("passes the dialog opener through to the last stop", async () => {
    const user = userEvent.setup();
    const onCreateProject = vi.fn();
    renderSeam({ profile: { is_tour_completed: false }, me: { first_name: "Ada" }, onCreateProject });
    await user.click(screen.getByRole("button", { name: "Take the tour" }));
    await user.click(screen.getByRole("button", { name: "Pages" }));
    await user.click(screen.getByRole("button", { name: "Start your first project" }));
    expect(onCreateProject).toHaveBeenCalledTimes(1);
  });
});
