// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant tab entry (parity/specs/profile.md §5 seam, §10): the shell
// mounts this wrapper, which keeps its own chunk out of the shell and
// shows a quiet frame until that chunk arrives.
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { AssistantSettingsTab, AssistantSettingsTabFallback } from "./AssistantSettingsTab.js";

beforeEach(() => {
  createAppClient("https://api.test", (() => new Promise<never>(() => undefined)) as Transport);
});

afterEach(() => resetAppClient());

function wrapper(): React.ReactElement {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <AssistantSettingsTab />
    </QueryClientProvider>
  );
}

describe("AssistantSettingsTab", () => {
  it("shows a quiet frame while its chunk loads", () => {
    render(<AssistantSettingsTabFallback />);
    const frame = screen.getByTestId("assistant-settings-loading");
    expect(frame).toHaveAttribute("aria-busy", "true");
    expect(frame.children.length).toBe(3);
  });

  it("shows its quiet frame before the chunk arrives", () => {
    render(wrapper());
    expect(screen.queryByTestId("assistant-settings-page")).not.toBeInTheDocument();
  });

  it("mounts the assistant forms once loaded", async () => {
    render(wrapper());
    expect(await screen.findByTestId("assistant-settings-loading", undefined, { timeout: 20000 })).toHaveAttribute(
      "aria-busy",
      "true"
    );
  });
});
