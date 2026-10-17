// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createClient, type State } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import { intakeStatesQueryOptions, selectIntakeFallback, useIntakeStates } from "./intakeOptions.js";

const TRIAGE: State = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  project_id: "223e4567-e89b-12d3-a456-426614174001",
  workspace_id: "323e4567-e89b-12d3-a456-426614174002",
  name: "Triage",
  color: "#5e6ad2",
  group: "triage",
  default: false,
  description: "",
  sequence: 1,
};

const SECOND: State = { ...TRIAGE, id: "423e4567-e89b-12d3-a456-426614174003", name: "Second" };

function stubTransport(status: number, body: unknown) {
  return (async (url: string) => ({
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  })) as never;
}

describe("selectIntakeFallback", () => {
  it("falls back to the first state, never the default flag", () => {
    expect(selectIntakeFallback([TRIAGE, SECOND])).toBe(TRIAGE);
    expect(selectIntakeFallback([])).toBe(null);
  });
});

describe("intakeStatesQueryOptions", () => {
  async function runQueryFn(status: number, body: unknown): Promise<State[]> {
    const client = createClient({ baseUrl: "https://api.test", transport: stubTransport(status, body) });
    const options = intakeStatesQueryOptions(client, "acme", "project-1");
    expect(options.queryKey).toEqual(issueKeys.intakeStates("acme", "project-1"));
    const queryFn = options.queryFn;
    if (queryFn === undefined) throw new Error("intake queryFn is missing");
    return queryFn({} as never);
  }

  it("wraps the single triage state into a list", async () => {
    await expect(runQueryFn(200, TRIAGE)).resolves.toEqual([TRIAGE]);
  });

  it("reads a missing triage state as empty", async () => {
    await expect(runQueryFn(404, {})).resolves.toEqual([]);
  });

  it("rethrows failures other than a missing triage state", async () => {
    await expect(runQueryFn(500, {})).rejects.toThrow();
  });
});

function Probe({ slug, projectId }: { slug: string; projectId: string }): React.ReactElement {
  const { states, isPending } = useIntakeStates(slug, projectId);
  return (
    <span data-testid="intake-states" data-pending={isPending ? "true" : "false"}>
      {(states ?? []).map((state) => state.name).join(",")}
    </span>
  );
}

describe("useIntakeStates", () => {
  afterEach(() => {
    resetAppClient();
  });

  it("serves the cached states", () => {
    createAppClient("https://api.test", (() => {
      throw new Error("must not fetch: states are cached");
    }) as never);
    const queryClient = new QueryClient();
    queryClient.setQueryData(issueKeys.intakeStates("acme", "project-1"), [TRIAGE]);
    render(
      <QueryClientProvider client={queryClient}>
        <Probe slug="acme" projectId="project-1" />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("intake-states")).toHaveTextContent("Triage");
  });

  it("stays pending while the states load", () => {
    createAppClient("https://api.test", (async () => {
      throw new Error("offline");
    }) as never);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Probe slug="acme" projectId="project-1" />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("intake-states")).toHaveAttribute("data-pending", "true");
  });
});
