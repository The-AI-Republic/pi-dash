// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail subscribe toggle (ISS-161): loader until known, hidden on
// archived items, disabled for guests and in flight, toasting on change.
import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { createQueryClient } from "../../../core/query/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  IssueSubscribeToggle,
  SUBSCRIBED_TITLE,
  SUBSCRIPTION_FAILED_TITLE,
  UNSUBSCRIBED_TITLE,
} from "./IssueSubscribeToggle.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

interface Gate {
  release: () => void;
}

function transport(state: { subscribed: boolean; failGet?: boolean; failWrite?: boolean; gate?: Gate }) {
  const calls: string[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push(`${method} ${url}`);
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (!url.endsWith("/subscribe/")) return json(404, {});
    if (method === "GET") {
      if (state.failGet === true) return json(400, { detail: "no" });
      return json(200, { subscribed: state.subscribed });
    }
    if (state.failWrite === true) return json(400, { detail: "no" });
    const gate = state.gate;
    if (gate !== undefined) {
      await new Promise<void>((resolve) => {
        gate.release = resolve;
      });
    }
    if (method === "POST") {
      state.subscribed = true;
      return json(201, {
        id: "723e4567-e89b-12d3-a456-426614174007",
        subscriber: "823e4567-e89b-12d3-a456-426614174008",
      });
    }
    state.subscribed = false;
    return json(204, {});
  }) as never);
  return { calls };
}

function renderToggle(props: Partial<React.ComponentProps<typeof IssueSubscribeToggle>> = {}) {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <IssueSubscribeToggle
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issueId={ISSUE}
        archived={false}
        canWrite
        {...props}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  resetAppClient();
});

afterEach(() => {
  cleanup();
  resetAppClient();
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
});

describe("IssueSubscribeToggle", () => {
  it("shows a loader until the state is known", () => {
    transport({ subscribed: false });
    renderToggle();
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(screen.getByText("Checking subscription")).toBeInTheDocument();
  });

  it("hides on archived items without a request", () => {
    const { calls } = transport({ subscribed: false });
    const { container } = renderToggle({ archived: true });
    expect(container).toBeEmptyDOMElement();
    expect(calls).toHaveLength(0);
  });

  it("hides when the read is refused", async () => {
    transport({ subscribed: false, failGet: true });
    const { container } = renderToggle();
    await waitFor(() => expect(container).toBeEmptyDOMElement());
  });

  it("subscribes with a toast and tells the detail", async () => {
    const state = { subscribed: false };
    transport(state);
    const onChanged = vi.fn();
    renderToggle({ onChanged });
    fireEvent.click(await screen.findByRole("button", { name: "Subscribe" }));
    await waitFor(() => expect(vi.mocked(notifySuccess)).toHaveBeenCalledWith(SUBSCRIBED_TITLE));
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("button", { name: "Unsubscribe" })).toBeInTheDocument();
  });

  it("unsubscribes without a detail callback", async () => {
    const state = { subscribed: true };
    transport(state);
    renderToggle();
    fireEvent.click(await screen.findByRole("button", { name: "Unsubscribe" }));
    await waitFor(() => expect(vi.mocked(notifySuccess)).toHaveBeenCalledWith(UNSUBSCRIBED_TITLE));
    expect(await screen.findByRole("button", { name: "Subscribe" })).toBeInTheDocument();
  });

  it("disables for guests", async () => {
    transport({ subscribed: false });
    renderToggle({ canWrite: false });
    expect(await screen.findByRole("button", { name: "Subscribe" })).toBeDisabled();
  });

  it("disables while a write is in flight", async () => {
    const state = { subscribed: false, gate: { release: () => undefined } as Gate };
    transport(state);
    renderToggle();
    const toggle = await screen.findByRole("button", { name: "Subscribe" });
    fireEvent.click(toggle);
    await waitFor(() => expect(screen.getByRole("button", { name: "Subscribe" })).toBeDisabled());
    state.gate.release();
    await waitFor(() => expect(screen.getByRole("button", { name: "Unsubscribe" })).toBeEnabled());
  });

  it("notices a refused write", async () => {
    transport({ subscribed: false, failWrite: true });
    renderToggle();
    fireEvent.click(await screen.findByRole("button", { name: "Subscribe" }));
    await waitFor(() =>
      expect(vi.mocked(notifyError)).toHaveBeenCalledWith(SUBSCRIPTION_FAILED_TITLE, expect.any(String))
    );
  });
});
