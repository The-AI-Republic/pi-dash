// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Integrations tab entry (parity/specs/profile.md §5 seam, §11): the
// shell mounts this wrapper, which loads the integrations body on demand.
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Transport } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useRouterState: ({ select }: { select: (state: { location: { searchStr: string; pathname: string } }) => unknown }) =>
    select({ location: { searchStr: "", pathname: "/settings/profile/integrations" } }),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { IntegrationsSettingsTab, IntegrationsSettingsTabFallback } from "./IntegrationsSettingsTab.js";

beforeEach(() => {
  createAppClient("https://api.test", (() => new Promise<never>(() => undefined)) as Transport);
});

afterEach(() => resetAppClient());

describe("IntegrationsSettingsTab", () => {
  it("shows a quiet frame while its chunk loads", () => {
    render(<IntegrationsSettingsTabFallback />);
    const frame = screen.getByTestId("integrations-settings-loading");
    expect(frame).toHaveAttribute("aria-busy", "true");
    expect(frame.children.length).toBe(3);
  });

  it("shows its quiet frame before the chunk arrives", () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <IntegrationsSettingsTab />
      </QueryClientProvider>
    );
    expect(screen.queryByTestId("integrations-settings-page")).not.toBeInTheDocument();
  });

  it("mounts the integrations body once loaded", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <IntegrationsSettingsTab />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("integrations-settings-page", undefined, { timeout: 20000 })).toBeInTheDocument();
  });
});
