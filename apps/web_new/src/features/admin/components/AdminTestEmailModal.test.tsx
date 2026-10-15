// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  AdminTestEmailModal,
  TEST_EMAIL_COMPOSE_TITLE,
  TEST_EMAIL_FAILED_TITLE,
  TEST_EMAIL_SUCCESS_TITLE,
} from "./AdminTestEmailModal.js";

function stubTransport(routes: Record<string, { status: number; body: unknown }>): Transport {
  return (url) => {
    const path = new URL(url, "https://api.test").pathname;
    const route = routes[path] ?? { status: 404, body: { detail: "not found" } };
    return Promise.resolve({
      status: route.status,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(route.body)),
    });
  };
}

function renderModal(onClose: () => void): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AdminTestEmailModal onClose={onClose} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("AdminTestEmailModal", () => {
  it("composes a receiver, sends, and lands on the success step", async () => {
    const calls: unknown[] = [];
    createAppClient("https://api.test", ((url: string, options?: { body?: string }) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/api/instances/email-credentials-check/") {
        calls.push(options?.body === undefined ? undefined : JSON.parse(options.body));
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ message: "Email successfully sent." })),
        });
      }
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    const onClose = vi.fn();
    renderModal(onClose);

    expect(screen.getByTestId("admin-test-email-step")).toHaveAttribute("data-step", "compose");
    expect(screen.getByRole("dialog")).toHaveAccessibleName(TEST_EMAIL_COMPOSE_TITLE);
    expect(screen.getByTestId("admin-test-email-receiver")).toHaveValue("");
    expect(screen.getByTestId("admin-test-email-dismiss")).toHaveTextContent("Cancel");

    fireEvent.change(screen.getByTestId("admin-test-email-receiver"), {
      target: { value: "admin@example.com" },
    });
    fireEvent.click(screen.getByTestId("admin-test-email-send"));
    await waitFor(() => expect(screen.getByTestId("admin-test-email-step")).toHaveAttribute("data-step", "success"));
    expect(screen.getByRole("dialog")).toHaveAccessibleName(TEST_EMAIL_SUCCESS_TITLE);
    expect(screen.getByTestId("admin-test-email-body")).toHaveTextContent("admin@example.com");
    expect(screen.getByTestId("admin-test-email-body")).toHaveTextContent("spam");
    expect(screen.queryByTestId("admin-test-email-send")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-test-email-dismiss")).toHaveTextContent("Close");
    // The check POST carries only the receiver address.
    expect(calls).toEqual([{ receiver_email: "admin@example.com" }]);

    fireEvent.click(screen.getByTestId("admin-test-email-dismiss"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows the server message on the failure step", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/email-credentials-check/": { status: 400, body: { error: "Receiver email is required" } },
      })
    );
    renderModal(vi.fn());

    fireEvent.click(screen.getByTestId("admin-test-email-send"));
    await waitFor(() => expect(screen.getByTestId("admin-test-email-step")).toHaveAttribute("data-step", "failed"));
    expect(screen.getByRole("dialog")).toHaveAccessibleName(TEST_EMAIL_FAILED_TITLE);
    expect(screen.getByTestId("admin-test-email-body")).toHaveTextContent("Receiver email is required");
    expect(screen.queryByTestId("admin-test-email-send")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-test-email-dismiss")).toHaveTextContent("Close");
  });

  it("closes on Escape through the dialog dismiss path", async () => {
    createAppClient("https://api.test", stubTransport({}));
    const onClose = vi.fn();
    renderModal(onClose);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
