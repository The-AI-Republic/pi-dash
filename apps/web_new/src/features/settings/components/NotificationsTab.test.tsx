// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notifications tab body (parity/specs/profile.md §8, PROF-018): the
// settings seam mounts the notifications area's own page on demand, so
// the shell never carries the preference code.
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

import { NotificationsTab } from "./NotificationsTab.js";

beforeEach(() => {
  createAppClient("https://api.test", () => new Promise<never>(() => undefined));
});

afterEach(() => resetAppClient());

describe("NotificationsTab", () => {
  it("renders nothing of the notifications area until its chunk arrives", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <NotificationsTab />
      </QueryClientProvider>
    );
    expect(screen.queryByTestId("email-prefs-loader")).not.toBeInTheDocument();
  });

  it("mounts the email preferences page once loaded", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <NotificationsTab />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("email-prefs-loader", undefined, { timeout: 20000 })).toBeInTheDocument();
  });
});
