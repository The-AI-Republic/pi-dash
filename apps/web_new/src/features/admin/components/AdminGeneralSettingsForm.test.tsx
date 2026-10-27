// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { InstanceDetails, Transport, TransportInit } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifySuccess: vi.fn() };
});

import { notifySuccess } from "../../../shared/shell/index.js";
import {
  GENERAL_NAME_REQUIRED_MESSAGE,
  GENERAL_SAVE_BUSY_LABEL,
  GENERAL_SAVE_FAILURE_LOG,
  GENERAL_SAVE_IDLE_LABEL,
  GENERAL_SAVE_SUCCESS_MESSAGE,
  GENERAL_SAVE_SUCCESS_TITLE,
  GENERAL_TELEMETRY_DOC_URL,
} from "../generalSettings.js";
import { AdminGeneralSettingsForm } from "./AdminGeneralSettingsForm.js";

function storedInstance(overrides: Partial<InstanceDetails> = {}): InstanceDetails {
  return {
    id: "55555555-5555-4555-8555-555555555555",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    instance_name: "Pi Dash",
    whitelist_emails: null,
    instance_id: "instance-1",
    current_version: "1.0.0",
    latest_version: null,
    edition: "oss",
    domain: "example.test",
    last_checked_at: "2026-01-01T00:00:00Z",
    namespace: null,
    is_telemetry_enabled: true,
    is_support_required: false,
    is_setup_done: true,
    is_signup_screen_visited: true,
    is_verified: false,
    is_test: false,
    is_current_version_deprecated: false,
    ...overrides,
  };
}

interface SeenWrite {
  body: unknown;
}

interface FormSetup {
  writes: SeenWrite[];
  patchStatus: number;
  holdPatch: boolean;
}

/** Resolve the held PATCH once the test has asserted the busy state. */
let releasePatch: (() => void) | null = null;

function renderForm(instance: InstanceDetails, setup: Partial<FormSetup> & { writes: SeenWrite[] }): void {
  const patchStatus = setup.patchStatus ?? 200;
  const transport: Transport = async (url: string, init: TransportInit) => {
    const path = new URL(url, "https://api.test").pathname;
    if (path === "/auth/get-csrf-token/") {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf-1" })),
      };
    }
    if (path === "/api/instances/" && init.method === "PATCH") {
      setup.writes.push({ body: JSON.parse(init.body ?? "{}") });
      if (setup.holdPatch === true) {
        await new Promise<void>((resolve) => {
          releasePatch = resolve;
        });
      }
      return {
        status: patchStatus,
        url,
        headers: { get: () => null },
        text: () =>
          Promise.resolve(
            patchStatus === 200
              ? JSON.stringify({ ...storedInstance(), updated_at: "2026-02-02T00:00:00Z" })
              : JSON.stringify({ detail: "boom" })
          ),
      };
    }
    return {
      status: 404,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
    };
  };
  createAppClient("https://api.test", transport);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AdminGeneralSettingsForm instance={instance} adminEmail="ada@example.com" />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  releasePatch = null;
  vi.mocked(notifySuccess).mockClear();
  vi.restoreAllMocks();
});

describe("AdminGeneralSettingsForm", () => {
  it("prefills the fields and locks the read-only pair", () => {
    renderForm(storedInstance(), { writes: [] });
    const name = screen.getByRole("textbox", { name: "Instance name" });
    expect(name).toHaveValue("Pi Dash");
    expect(name).toBeEnabled();
    const email = screen.getByRole("textbox", { name: "Administrator email" });
    expect(email).toHaveValue("ada@example.com");
    expect(email).toBeDisabled();
    const instanceId = screen.getByRole("textbox", { name: "Instance ID" });
    expect(instanceId).toHaveValue("instance-1");
    expect(instanceId).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Usage telemetry" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL })).toBeEnabled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("links the telemetry policy externally", () => {
    renderForm(storedInstance(), { writes: [] });
    const link = screen.getByRole("link", { name: "Learn more" });
    expect(link.getAttribute("href")).toBe(GENERAL_TELEMETRY_DOC_URL);
    expect(link.getAttribute("target")).toBe("_blank");
  });

  it("blocks an empty save with an inline required message and no write", async () => {
    const user = userEvent.setup();
    const writes: SeenWrite[] = [];
    renderForm(storedInstance(), { writes });
    await user.clear(screen.getByRole("textbox", { name: "Instance name" }));
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL }));
    expect(screen.getByRole("alert")).toHaveTextContent(GENERAL_NAME_REQUIRED_MESSAGE);
    expect(writes).toEqual([]);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL })).toBeEnabled();
  });

  it("blocks a whitespace-only save the same way", async () => {
    const user = userEvent.setup();
    const writes: SeenWrite[] = [];
    renderForm(storedInstance(), { writes });
    await user.clear(screen.getByRole("textbox", { name: "Instance name" }));
    await user.type(screen.getByRole("textbox", { name: "Instance name" }), "   ");
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL }));
    expect(screen.getByRole("alert")).toHaveTextContent(GENERAL_NAME_REQUIRED_MESSAGE);
    expect(writes).toEqual([]);
  });

  it("wires the required message to the name field the way the parity driver reads it", async () => {
    const user = userEvent.setup();
    const writes: SeenWrite[] = [];
    renderForm(storedInstance(), { writes });
    const name = screen.getByRole("textbox", { name: "Instance name" });
    // Before any save attempt the field describes nothing: no message
    // shows while the prefilled name is valid.
    expect(name).not.toHaveAttribute("aria-describedby");
    expect(name).not.toHaveAttribute("aria-invalid");
    await user.clear(name);
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL }));
    // The parity driver resolves the field's message through
    // aria-describedby and reads its visible text, so the mapping must
    // point at the rendered required message, not just a sibling alert.
    expect(name).toHaveAttribute("aria-invalid", "true");
    const describedBy = name.getAttribute("aria-describedby");
    expect(describedBy).not.toBeNull();
    const message = document.getElementById(describedBy ?? "");
    expect(message).not.toBeNull();
    expect(message).toHaveTextContent(GENERAL_NAME_REQUIRED_MESSAGE);
    expect(message?.getAttribute("role")).toBe("alert");
    expect(writes).toEqual([]);
  });

  it("clears the required message once the name turns valid", async () => {
    const user = userEvent.setup();
    renderForm(storedInstance(), { writes: [] });
    const name = screen.getByRole("textbox", { name: "Instance name" });
    await user.clear(name);
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL }));
    expect(screen.getByRole("alert")).toHaveTextContent(GENERAL_NAME_REQUIRED_MESSAGE);
    await user.type(name, "Pi Dash");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("saves a rename with the telemetry flag and raises the notice", async () => {
    const user = userEvent.setup();
    const writes: SeenWrite[] = [];
    renderForm(storedInstance(), { writes });
    await user.clear(screen.getByRole("textbox", { name: "Instance name" }));
    await user.type(screen.getByRole("textbox", { name: "Instance name" }), "Renamed");
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(Object.keys((writes[0]?.body ?? {}) as Record<string, unknown>).sort()).toEqual([
      "instance_name",
      "is_telemetry_enabled",
    ]);
    expect(writes[0]?.body).toEqual({ instance_name: "Renamed", is_telemetry_enabled: true });
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(GENERAL_SAVE_SUCCESS_TITLE, GENERAL_SAVE_SUCCESS_MESSAGE)
    );
    expect(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL })).toBeEnabled();
  });

  it("holds a telemetry flip locally until the shared save", async () => {
    const user = userEvent.setup();
    const writes: SeenWrite[] = [];
    renderForm(storedInstance(), { writes });
    const toggle = screen.getByRole("switch", { name: "Usage telemetry" });
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(writes).toEqual([]);
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]?.body).toEqual({ instance_name: "Pi Dash", is_telemetry_enabled: false });
  });

  it("swaps the label and locks the switch while the save is in flight", async () => {
    const user = userEvent.setup();
    const writes: SeenWrite[] = [];
    renderForm(storedInstance(), { writes, holdPatch: true });
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL }));
    await waitFor(() => expect(screen.getByRole("button", { name: GENERAL_SAVE_BUSY_LABEL })).toBeDisabled());
    expect(screen.getByRole("switch", { name: "Usage telemetry" })).toBeDisabled();
    releasePatch?.();
    await waitFor(() => expect(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL })).toBeEnabled());
    expect(screen.getByRole("switch", { name: "Usage telemetry" })).toBeEnabled();
    expect(writes).toHaveLength(1);
    expect(notifySuccess).toHaveBeenCalledTimes(1);
  });

  it("logs a failed save with no notice and returns to idle", async () => {
    const user = userEvent.setup();
    const errors: unknown[][] = [];
    vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      errors.push(args);
    });
    const writes: SeenWrite[] = [];
    renderForm(storedInstance(), { writes, patchStatus: 500 });
    await user.clear(screen.getByRole("textbox", { name: "Instance name" }));
    await user.type(screen.getByRole("textbox", { name: "Instance name" }), "Failure Probe");
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL }));
    await waitFor(() => expect(writes).toHaveLength(1));
    await waitFor(() => expect(errors).toHaveLength(1));
    expect(errors[0]?.[0]).toBe(GENERAL_SAVE_FAILURE_LOG);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: GENERAL_SAVE_IDLE_LABEL })).toBeEnabled();
  });
});
