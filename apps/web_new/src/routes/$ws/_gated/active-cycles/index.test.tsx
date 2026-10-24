// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace active-cycles route (CYC-043). Thin mount: proves the
// route renders the paid upsell for the workspace slug and issues no
// requests — the active-cycles endpoint is never read here.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";

import { Route } from "./index.js";

let requests: number;

beforeEach(() => {
  requests = 0;
  createAppClient("https://api.test", (() => {
    requests += 1;
    return new Promise<never>(() => undefined);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("active-cycles route", () => {
  it("mounts the upsell for the workspace without reading", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    const Component = Route.options.component as () => React.JSX.Element;
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Component />
      </QueryClientProvider>
    );
    try {
      expect(await screen.findByTestId("workspace-actives-upsell")).toBeInTheDocument();
      expect(requests).toBe(0);
    } finally {
      unmount();
    }
  });
});
