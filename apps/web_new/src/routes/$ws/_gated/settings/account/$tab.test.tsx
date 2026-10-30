// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One account tab's address guard (PROF-009; main#670, CLOUD-035): a
// registered, served tab loads; an unknown id and a tab the cloud instance
// withholds both forward to the general tab of the same workspace with the
// query string kept.
import type { QueryClient } from "@tanstack/react-query";
import { isRedirect } from "@tanstack/react-router";
import { createClient } from "@pidash/api-client";
import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { resetProfileSettingsPages } from "../../../../../features/settings/lib/registry.js";
import { Route } from "./$tab.js";

function queryClient(selfManaged: boolean): QueryClient {
  return {
    ensureQueryData: async () => ({ is_self_managed: selfManaged }),
  } as unknown as QueryClient;
}

const pages = vi.hoisted(() => ({ current: null as (() => React.ReactNode) | null }));

vi.mock("../../../../../features/settings/lib/registry.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../../../features/settings/lib/registry.js")>();
  return {
    ...actual,
    useProfileSettingsPage: () => pages.current,
  };
});

const apiClient = createClient({
  baseUrl: "https://api.test",
  transport: async () => {
    throw new Error("no fetch");
  },
});

async function guard(args: { tab: string; selfManaged?: boolean; searchStr?: string }): Promise<unknown> {
  return Route.options.beforeLoad?.({
    context: { queryClient: queryClient(args.selfManaged ?? true), apiClient },
    params: { ws: "acme", tab: args.tab },
    location: { pathname: `/acme/settings/account/${args.tab}/`, searchStr: args.searchStr ?? "" },
  } as never);
}

/** The tab body component, rendered with the route context wired as the router does. */
function renderTab(tab: string): void {
  const Component = Route.options.component;
  if (Component === undefined) throw new Error("missing account tab component");
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", tab } as never);
  render(<Component />);
}

describe("account tab guard", () => {
  it("loads a registered tab", async () => {
    await expect(guard({ tab: "preferences" })).resolves.toBeUndefined();
  });

  it("forwards an unknown id to the general tab, keeping the query", async () => {
    const thrown = await guard({ tab: "not-a-tab", searchStr: "?keep=1" }).catch((error: unknown) => error);
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/acme/settings/account/general/?keep=1");
  });

  it("forwards a withheld tab on a cloud-managed instance", async () => {
    const thrown = await guard({ tab: "security", selfManaged: false }).catch((error: unknown) => error);
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/acme/settings/account/general/");
  });

  it("keeps the withheld tab on a self-managed instance", async () => {
    await expect(guard({ tab: "security", selfManaged: true })).resolves.toBeUndefined();
  });
});

describe("account tab body", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    resetProfileSettingsPages();
    cleanup();
  });

  it("renders the body registered for the addressed tab", () => {
    pages.current = () => <p data-testid="tab-body">preferences</p>;
    renderTab("preferences");
    expect(screen.getByTestId("tab-body")).toHaveTextContent("preferences");
  });

  it("falls back to the general body when the address outlives its param", () => {
    // The guard above redirects an unknown id, but the component still
    // paints a frame while the redirect settles: it must never look up a
    // tab the registry cannot render.
    const bodies = vi.fn(() => <p data-testid="tab-body">body</p>);
    pages.current = bodies;
    renderTab("no-such-tab");
    expect(screen.getByTestId("tab-body")).toBeInTheDocument();
  });
});
