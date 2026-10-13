// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DEVICE_APPROVE_FAILED_MESSAGE } from "../device.js";
import { DeviceApprovalCard } from "./DeviceApprovalCard.js";

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string) => StubRoute | Promise<StubRoute>): Transport {
  return async (url) => {
    const response = await handler(url);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });
const APPROVE_OK = JSON.stringify({ ok: true, user_email: "ada@example.com", workspace_slug: "acme" });

function renderCard(options: {
  approve?: (url: string) => StubRoute | Promise<StubRoute>;
  initialCode?: string | undefined;
  strict?: boolean | undefined;
}): { approvePosts: string[] } {
  const approvePosts: string[] = [];
  const transport = stubTransport((url) => {
    if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, url, body: CSRF };
    if (url.endsWith("/api/v1/auth/device/approve/")) {
      approvePosts.push(url);
      if (options.approve) return options.approve(url);
      return { status: 200, url, body: APPROVE_OK };
    }
    return { status: 404, url, body: "{}" };
  });
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const card = <DeviceApprovalCard initialCode={options.initialCode} />;
  render(
    <QueryClientProvider client={queryClient}>
      {options.strict ? <React.StrictMode>{card}</React.StrictMode> : card}
    </QueryClientProvider>
  );
  return { approvePosts };
}

afterEach(() => {
  resetAppClient();
});

describe("DeviceApprovalCard", () => {
  it("autofocuses the code field and formats as typed", async () => {
    renderCard({});
    const field = screen.getByPlaceholderText("XXXX-YYYY");
    expect(field).toHaveFocus();

    const user = userEvent.setup();
    await user.type(field, "abcd1234");
    expect(field).toHaveValue("ABCD-1234");
  });

  it("enables submit only for complete codes", async () => {
    renderCard({});
    const user = userEvent.setup();
    const field = screen.getByPlaceholderText("XXXX-YYYY");
    const submit = screen.getByRole("button", { name: "Approve" });

    expect(submit).toBeDisabled();
    await user.type(field, "ab12");
    expect(submit).toBeDisabled();
    await user.type(field, "3456");
    expect(submit).toBeEnabled();
  });

  it("submits with Enter and confirms with the account and workspace", async () => {
    renderCard({});
    const user = userEvent.setup();
    await user.type(screen.getByPlaceholderText("XXXX-YYYY"), "abcd1234");
    await user.keyboard("{Enter}");

    await waitFor(() => expect(screen.queryByPlaceholderText("XXXX-YYYY")).not.toBeInTheDocument());
    expect(screen.getByText("ada@example.com")).toBeVisible();
    expect(screen.getByText("acme")).toBeVisible();
  });

  it("omits the workspace row when the grant names none", async () => {
    renderCard({
      approve: (url) => ({
        status: 200,
        url,
        body: JSON.stringify({ ok: true, user_email: "ada@example.com", workspace_slug: null }),
      }),
    });
    const user = userEvent.setup();
    await user.type(screen.getByPlaceholderText("XXXX-YYYY"), "abcd1234");
    await user.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(screen.getByText("ada@example.com")).toBeVisible());
    expect(screen.queryByText("Workspace")).not.toBeInTheDocument();
  });

  it("keeps the form editable after a failure and retries", async () => {
    const seen = renderCard({
      approve: (url) => ({ status: 404, url, body: JSON.stringify({ error: "Code not recognized." }) }),
    });
    const user = userEvent.setup();
    await user.type(screen.getByPlaceholderText("XXXX-YYYY"), "zzzzzzzz");
    await user.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(screen.getByText("Code not recognized.")).toBeVisible());
    const field = screen.getByPlaceholderText("XXXX-YYYY");
    expect(field).toHaveValue("ZZZZ-ZZZZ");
    await user.clear(field);
    await user.type(field, "abcd1234");
    await user.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(seen.approvePosts).toHaveLength(2));
  });

  it("falls back to the retry line without a server detail", async () => {
    renderCard({
      approve: () => {
        throw new Error("");
      },
    });
    const user = userEvent.setup();
    await user.type(screen.getByPlaceholderText("XXXX-YYYY"), "abcd1234");
    await user.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(screen.getByText(DEVICE_APPROVE_FAILED_MESSAGE)).toBeVisible());
  });

  it("disables submit while approval is in flight", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderCard({
      approve: async (url) => {
        await gate;
        return { status: 200, url, body: APPROVE_OK };
      },
    });
    const user = userEvent.setup();
    await user.type(screen.getByPlaceholderText("XXXX-YYYY"), "abcd1234");
    await user.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "Approving…" })).toBeDisabled());
    release();
    await waitFor(() => expect(screen.getByText("ada@example.com")).toBeVisible());
  });

  it("prefills a CLI-carried code and tries once on mount", async () => {
    const seen = renderCard({
      initialCode: "zzzz-yyyy",
      strict: true,
      approve: (url) => ({ status: 404, url, body: JSON.stringify({ error: "Code not recognized." }) }),
    });

    expect(screen.getByPlaceholderText("XXXX-YYYY")).toHaveValue("ZZZZ-YYYY");
    await waitFor(() => expect(screen.getByText("Code not recognized.")).toBeVisible());
    expect(seen.approvePosts).toHaveLength(1);

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(seen.approvePosts).toHaveLength(2));
  });

  it("prefills short CLI-carried codes without an auto-attempt", async () => {
    const seen = renderCard({ initialCode: "abc" });
    expect(screen.getByPlaceholderText("XXXX-YYYY")).toHaveValue("ABC");
    const submit = await screen.findByRole("button", { name: "Approve" });
    expect(submit).toBeDisabled();
    expect(seen.approvePosts).toHaveLength(0);
  });
});
