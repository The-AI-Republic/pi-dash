// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { EstimateList } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import { formatEstimatePoint, useActiveEstimateSystem } from "./estimateOptions.js";

describe("formatEstimatePoint", () => {
  it("renders points and categories raw", () => {
    expect(formatEstimatePoint("3", "points")).toBe("3");
    expect(formatEstimatePoint("XL", "category")).toBe("XL");
  });

  it("renders TIME minutes below the hour", () => {
    expect(formatEstimatePoint("0", "time")).toBe("0m");
    expect(formatEstimatePoint("30", "time")).toBe("30m");
  });

  it("renders TIME hours with and without a minutes part", () => {
    expect(formatEstimatePoint("90", "time")).toBe("1h 30m");
    expect(formatEstimatePoint("120", "time")).toBe("2h");
    expect(formatEstimatePoint("150", "time")).toBe("2h 30m");
  });

  it("renders unparseable TIME values raw", () => {
    expect(formatEstimatePoint("soon", "time")).toBe("soon");
    expect(formatEstimatePoint("1.5", "time")).toBe("1.5");
    expect(formatEstimatePoint("-30", "time")).toBe("-30");
    expect(formatEstimatePoint("", "time")).toBe("");
    expect(formatEstimatePoint("  ", "time")).toBe("  ");
  });
});

const SYSTEM_A = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  name: "Points",
  type: "points",
  last_used: false,
  points: [],
};

const SYSTEM_B = {
  ...SYSTEM_A,
  id: "223e4567-e89b-12d3-a456-426614174001",
  name: "Time",
  type: "time",
  last_used: true,
};

function Probe({ slug, projectId }: { slug: string; projectId: string }): React.ReactElement {
  const { system, isPending } = useActiveEstimateSystem(slug, projectId);
  return (
    <span data-testid="estimate-system" data-pending={isPending ? "true" : "false"}>
      {system?.name ?? "none"}
    </span>
  );
}

describe("useActiveEstimateSystem", () => {
  afterEach(() => {
    resetAppClient();
  });

  function renderProbe(queryClient: QueryClient, systems?: EstimateList): void {
    if (systems !== undefined) queryClient.setQueryData(issueKeys.estimates("acme", "project-1"), systems);
    render(
      <QueryClientProvider client={queryClient}>
        <Probe slug="acme" projectId="project-1" />
      </QueryClientProvider>
    );
  }

  it("selects the active system from the cached list", () => {
    createAppClient("https://api.test", (() => {
      throw new Error("must not fetch: estimates are cached");
    }) as never);
    renderProbe(new QueryClient(), [SYSTEM_A, SYSTEM_B]);
    expect(screen.getByTestId("estimate-system")).toHaveTextContent("Time");
  });

  it("reports none without an active system", () => {
    createAppClient("https://api.test", (() => {
      throw new Error("must not fetch: estimates are cached");
    }) as never);
    renderProbe(new QueryClient(), [SYSTEM_A]);
    expect(screen.getByTestId("estimate-system")).toHaveTextContent("none");
  });

  it("stays pending while the list loads", () => {
    createAppClient("https://api.test", (async () => {
      throw new Error("offline");
    }) as never);
    renderProbe(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    expect(screen.getByTestId("estimate-system")).toHaveAttribute("data-pending", "true");
  });
});
