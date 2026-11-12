// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  INVITE_FAILED_TITLE,
  INVITE_ROW_ERROR_MESSAGE,
  INVITE_SUCCESS_TITLE,
  InviteMembersStep,
} from "./InviteMembersStep.js";

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

const WORKSPACE_BODY = {
  id: "333e4567-e89b-12d3-a456-426614174002",
  name: "Acme",
  slug: "acme",
  logo_url: null,
  total_members: 1,
  role: 20,
};

interface InviteHarness {
  advanced: number;
  posted: unknown[];
  seen: string[];
  queryClient: QueryClient;
}

function renderStep(options: {
  workspaces?: unknown[] | "unset";
  failSend?: boolean;
  gateSend?: { gate: Promise<StubRoute> };
}): InviteHarness {
  const harness: InviteHarness = {
    advanced: 0,
    posted: [],
    seen: [],
    queryClient: new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } }),
  };
  createAppClient(
    "https://api.test",
    stubTransport(async (url, init) => {
      harness.seen.push(`${init.method} ${url}`);
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/workspaces/acme/invitations/") && init.method === "POST") {
        harness.posted.push(JSON.parse(init.body as string));
        if (options.gateSend) return options.gateSend.gate;
        if (options.failSend) return { status: 400, body: JSON.stringify({ error: "bad row" }), url };
        return { status: 200, body: JSON.stringify({ message: "Invitations sent" }), url };
      }
      if (url.endsWith("/api/users/me/workspaces/")) {
        return { status: 200, body: "[]", url };
      }
      return { status: 200, body: "{}", url };
    })
  );
  if (options.workspaces !== "unset") {
    harness.queryClient.setQueryData(["session", "workspaces"], options.workspaces ?? [WORKSPACE_BODY]);
  }
  render(
    <QueryClientProvider client={harness.queryClient}>
      <InviteMembersStep
        onAdvance={() => {
          harness.advanced += 1;
        }}
      />
    </QueryClientProvider>
  );
  return harness;
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
});

describe("InviteMembersStep", () => {
  it("opens with three blank member rows and send disabled", () => {
    renderStep({});
    expect(screen.getByRole("heading", { name: "Invite your team" })).toBeInTheDocument();
    expect(screen.getByLabelText("Email address 1")).toHaveValue("");
    expect(screen.getByLabelText("Email address 2")).toHaveValue("");
    expect(screen.getByLabelText("Email address 3")).toHaveValue("");
    expect(screen.queryByLabelText("Email address 4")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Role 1")).toHaveValue("15");
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("grows on demand and hides removal on the last row", async () => {
    renderStep({});
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Add another" }));
    expect(screen.getByLabelText("Email address 4")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Remove invite 4" }));
    await user.click(screen.getByRole("button", { name: "Remove invite 3" }));
    await user.click(screen.getByRole("button", { name: "Remove invite 2" }));
    expect(screen.queryByLabelText("Email address 2")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Remove invite/ })).not.toBeInTheDocument();
  });

  it("flags malformed rows and sends only when a valid address exists", () => {
    renderStep({});
    fireEvent.change(screen.getByLabelText("Email address 1"), { target: { value: "not-an-email" } });
    expect(screen.getByText(INVITE_ROW_ERROR_MESSAGE)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();

    fireEvent.change(screen.getByLabelText("Email address 2"), { target: { value: "sam@example.com" } });
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();

    fireEvent.change(screen.getByLabelText("Email address 1"), { target: { value: "" } });
    expect(screen.queryByText(INVITE_ROW_ERROR_MESSAGE)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
  });

  it("drops empty rows, posts the filled ones with their roles, and advances", async () => {
    const harness = renderStep({});
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Email address 1"), { target: { value: "sam@example.com" } });
    fireEvent.change(screen.getByLabelText("Role 1"), { target: { value: "20" } });
    fireEvent.change(screen.getByLabelText("Email address 3"), { target: { value: "  jo@example.com " } });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(harness.advanced).toBe(1));
    expect(harness.posted).toEqual([
      {
        emails: [
          { email: "sam@example.com", role: 20 },
          { email: "jo@example.com", role: 15 },
        ],
      },
    ]);
    expect(harness.seen).toContain("POST https://api.test/api/workspaces/acme/invitations/");
    expect(notifySuccess).toHaveBeenCalledWith(INVITE_SUCCESS_TITLE);
  });

  it("never submits with Enter", async () => {
    const harness = renderStep({});
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Email address 1"), "sam@example.com{enter}");
    await waitFor(() => expect(screen.getByLabelText("Email address 1")).toHaveValue("sam@example.com"));
    expect(harness.posted).toHaveLength(0);
    expect(harness.advanced).toBe(0);
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(harness.advanced).toBe(1));
  });

  it("toasts the server detail when sending fails and stays on the step", async () => {
    const harness = renderStep({ failSend: true });
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Email address 1"), { target: { value: "sam@example.com" } });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(INVITE_FAILED_TITLE, expect.any(String));
    expect(harness.advanced).toBe(0);
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
  });

  it("defers without sending anything", async () => {
    const harness = renderStep({});
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Invite later" }));
    expect(harness.advanced).toBe(1);
    expect(harness.posted).toHaveLength(0);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("disables sending without a workspace and ignores programmatic submits", () => {
    const harness = renderStep({ workspaces: [] });
    fireEvent.change(screen.getByLabelText("Email address 1"), { target: { value: "sam@example.com" } });
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    fireEvent.submit(screen.getByRole("button", { name: "Continue" }).closest("form")!);
    expect(harness.posted).toHaveLength(0);
    expect(harness.advanced).toBe(0);
  });

  it("disables sending while workspaces load and ignores programmatic submits", () => {
    const harness = renderStep({ workspaces: "unset" });
    fireEvent.change(screen.getByLabelText("Email address 1"), { target: { value: "sam@example.com" } });
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    fireEvent.submit(screen.getByRole("button", { name: "Continue" }).closest("form")!);
    expect(harness.posted).toHaveLength(0);
    expect(harness.advanced).toBe(0);
  });

  it("ignores programmatic submits while rows are invalid", () => {
    const harness = renderStep({});
    fireEvent.submit(screen.getByRole("button", { name: "Continue" }).closest("form")!);
    expect(harness.posted).toHaveLength(0);
    expect(harness.advanced).toBe(0);
  });

  it("disables both actions while sending", async () => {
    let release!: (route: StubRoute) => void;
    const gate = new Promise<StubRoute>((resolve) => {
      release = resolve;
    });
    const harness = renderStep({ gateSend: { gate } });
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Email address 1"), { target: { value: "sam@example.com" } });
    const pending = user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Invite later" })).toBeDisabled());
    release({ status: 200, body: JSON.stringify({ message: "Invitations sent" }), url: "" });
    await pending;
    await waitFor(() => expect(harness.advanced).toBe(1));
  });
});
