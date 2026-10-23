// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));

const embed = vi.hoisted(() => ({ framed: false }));
vi.mock("../lib/spaceEmbed.js", () => ({ useSpaceEmbedded: () => embed.framed }));

import { SpaceAccountSlot } from "./SpaceAccountSlot.js";

const ME = {
  id: "11111111-1111-4111-8111-111111111111",
  email: "ada@example.test",
  username: "ada",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "Ada",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

function renderSlot(viewer: () => Awaited<ReturnType<Transport>> | Promise<never>): void {
  const transport: Transport = async (url) => {
    if (url.includes("/api/users/me/")) return viewer();
    throw new Error(`unexpected request ${url}`);
  };
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={runtime.queryClient as QueryClient}>
      <SpaceAccountSlot anchor="anchor-1" search={{ board: "kanban", peekId: "issue-1" }} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  embed.framed = false;
  vi.restoreAllMocks();
});

describe("SpaceAccountSlot", () => {
  it("renders nothing inside an embedding frame", () => {
    embed.framed = true;
    renderSlot(() => reply(200, ME));
    expect(screen.queryByTestId("space-navbar-signin")).not.toBeInTheDocument();
    expect(screen.queryByTestId("space-account-trigger")).not.toBeInTheDocument();
  });

  it("offers sign-in while identity loads", () => {
    renderSlot(() => new Promise<never>(() => undefined));
    expect(screen.getByTestId("space-navbar-signin")).toHaveAttribute(
      "href",
      "/spaces/?next_path=/issues/anchor-1/%3Fboard%3Dkanban%26peekId%3Dissue-1"
    );
    expect(screen.queryByTestId("space-account-trigger")).not.toBeInTheDocument();
  });

  it("offers sign-in carrying the full position to anonymous visitors", async () => {
    renderSlot(() => reply(401, { detail: "anonymous" }));
    const link = await screen.findByTestId("space-navbar-signin");
    expect(link).toHaveAttribute("href", "/spaces/?next_path=/issues/anchor-1/%3Fboard%3Dkanban%26peekId%3Dissue-1");
    expect(link).toHaveTextContent("Sign in");
  });

  it("offers the account menu to signed-in visitors", async () => {
    renderSlot(() => reply(200, ME));
    expect(await screen.findByTestId("space-account-trigger")).toHaveTextContent("Ada");
    expect(screen.queryByTestId("space-navbar-signin")).not.toBeInTheDocument();
  });
});
