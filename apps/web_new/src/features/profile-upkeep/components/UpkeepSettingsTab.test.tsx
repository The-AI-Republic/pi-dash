// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Upkeep tab entry (parity/specs/profile.md §5 seam, §11): the shell
// mounts this wrapper, which loads the upkeep body and its hooks on demand.
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { UpkeepSettingsTab, UpkeepSettingsTabFallback } from "./UpkeepSettingsTab.js";

beforeEach(() => {
  createAppClient("https://api.test", (() => new Promise<never>(() => undefined)) as Transport);
});

afterEach(() => resetAppClient());

function wrapper(): React.ReactElement {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <UpkeepSettingsTab />
    </QueryClientProvider>
  );
}

describe("UpkeepSettingsTab", () => {
  it("shows a quiet frame while its chunk loads", () => {
    render(<UpkeepSettingsTabFallback />);
    const frame = screen.getByTestId("upkeep-settings-loading");
    expect(frame).toHaveAttribute("aria-busy", "true");
    expect(frame.children.length).toBe(3);
  });

  it("shows its quiet frame before the chunk arrives", () => {
    render(wrapper());
    expect(screen.getByTestId("upkeep-settings-loading")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("upkeep-settings-page")).not.toBeInTheDocument();
  });

  it("mounts the upkeep body once loaded", async () => {
    render(wrapper());
    expect(await screen.findByTestId("upkeep-settings-loading", undefined, { timeout: 20000 })).toBeInTheDocument();
  });
});
