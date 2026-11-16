// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { useMeSettings, useProfile } from "./hooks.js";
import { setSessionClient } from "./queries.js";

function hookHarness(data: unknown) {
  setSessionClient({
    getJson: async () => data,
    parse: (_schema: unknown, payload: unknown) => payload,
  } as never);
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function ProfileProbe(): React.ReactElement {
  const query = useProfile();
  return <div data-testid="probe">{query.data === undefined ? "loading" : JSON.stringify(query.data)}</div>;
}

function SettingsProbe(): React.ReactElement {
  const query = useMeSettings();
  return <div data-testid="probe">{query.data === undefined ? "loading" : JSON.stringify(query.data)}</div>;
}

describe("useProfile", () => {
  it("reads the profile through the session client", async () => {
    const profile = { id: "profile-1", is_onboarded: true };
    render(
      <QueryClientProvider client={hookHarness(profile)}>
        <ProfileProbe />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent(JSON.stringify(profile)));
  });
});

describe("useMeSettings", () => {
  it("reads the settings through the session client", async () => {
    const settings = { id: "user-1", workspace: { last_workspace_slug: "acme" } };
    render(
      <QueryClientProvider client={hookHarness(settings)}>
        <SettingsProbe />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent(JSON.stringify(settings)));
  });
});
