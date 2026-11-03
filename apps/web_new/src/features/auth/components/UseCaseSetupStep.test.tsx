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
import { setSessionClient } from "../../../core/session/queries.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { combineUseCases, ONBOARDING_USE_CASES } from "../onboarding.js";
import {
  USE_CASE_REQUIRED_MESSAGE,
  USE_CASE_SAVE_FAILED_TITLE,
  USE_CASE_SAVE_SUCCESS_TITLE,
  UseCaseSetupStep,
} from "./UseCaseSetupStep.js";

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

const PROFILE_BODY = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  user: "123e4567-e89b-12d3-a456-426614174000",
  theme: {},
  is_app_rail_docked: false,
  is_tour_completed: false,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: false,
  last_workspace_id: null,
  billing_address_country: "",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "full",
  is_smooth_cursor_enabled: false,
  is_mobile_onboarded: false,
  mobile_onboarding_step: {},
  mobile_timezone_auto_set: false,
  language: "en",
  start_of_the_week: 0,
  goals: {},
  background_color: "",
  is_navigation_tour_completed: false,
  has_marketing_email_consent: false,
  is_subscribed_to_changelog: false,
  product_tour: {},
  settings: {},
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-02-03T04:05:06Z",
};

function renderStep(options: {
  useCase?: string | null;
  bodies?: Record<string, string | undefined>;
  failSave?: boolean;
  onAdvance?: () => void;
}): void {
  createAppClient(
    "https://api.test",
    stubTransport(async (url, init) => {
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/users/me/profile/") && init.method === "PATCH") {
        if (options.bodies) options.bodies["profile"] = init.body;
        if (options.failSave) return { status: 500, body: "{}", url };
        return { status: 200, body: JSON.stringify(PROFILE_BODY), url };
      }
      return { status: 200, body: "{}", url };
    })
  );
  setSessionClient({
    getJson: async () => ({}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(["session", "profile"], { ...PROFILE_BODY, use_case: options.useCase ?? null });
  render(
    <QueryClientProvider client={queryClient}>
      <UseCaseSetupStep onAdvance={options.onAdvance ?? (() => undefined)} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
});

describe("UseCaseSetupStep", () => {
  it("lists every shared use case with submit disabled", () => {
    renderStep({});
    expect(screen.getByRole("heading", { name: "What brings you to Pi Dash?" })).toBeInTheDocument();
    for (const useCase of ONBOARDING_USE_CASES) {
      expect(screen.getByRole("checkbox", { name: useCase })).toBeInTheDocument();
    }
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("prefills the stored combined value", () => {
    const first = ONBOARDING_USE_CASES[0] ?? "";
    const second = ONBOARDING_USE_CASES[1] ?? "";
    renderStep({ useCase: combineUseCases([first, second]) });
    expect(screen.getByRole("checkbox", { name: first })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: second })).toBeChecked();
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
  });

  it("toggles choices on and off", async () => {
    const user = userEvent.setup();
    renderStep({});
    const box = screen.getByRole("checkbox", { name: ONBOARDING_USE_CASES[0] ?? "" });
    await user.click(box);
    expect(box).toBeChecked();
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
    await user.click(box);
    expect(box).not.toBeChecked();
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("saves the selection combined and advances", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    const onAdvance = vi.fn();
    renderStep({ bodies, onAdvance });
    const first = ONBOARDING_USE_CASES[0] ?? "";
    const second = ONBOARDING_USE_CASES[2] ?? "";

    await user.click(screen.getByRole("checkbox", { name: first }));
    await user.click(screen.getByRole("checkbox", { name: second }));
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
    expect(JSON.parse(bodies["profile"] ?? "{}")).toEqual({ use_case: combineUseCases([first, second]) });
    expect(notifySuccess).toHaveBeenCalledWith(USE_CASE_SAVE_SUCCESS_TITLE);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("shows the required message on an empty submit", async () => {
    const user = userEvent.setup();
    renderStep({});
    const form = screen.getByRole("button", { name: "Continue" }).closest("form");
    expect(form).not.toBeNull();
    if (form) fireEvent.submit(form);
    expect(screen.getByText(USE_CASE_REQUIRED_MESSAGE)).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox", { name: ONBOARDING_USE_CASES[0] ?? "" }));
    expect(screen.queryByText(USE_CASE_REQUIRED_MESSAGE)).toBeNull();
  });

  it("starts unselected while the profile loads", () => {
    createAppClient(
      "https://api.test",
      stubTransport(async () => ({ status: 200, body: "{}", url: "" }))
    );
    setSessionClient({
      getJson: () => new Promise(() => undefined),
      parse: (_schema: unknown, data: unknown) => data,
    } as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <UseCaseSetupStep onAdvance={() => undefined} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("checkbox", { name: ONBOARDING_USE_CASES[0] ?? "" })).not.toBeChecked();
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("skips without storing", async () => {
    const user = userEvent.setup();
    const bodies: Record<string, string | undefined> = {};
    const onAdvance = vi.fn();
    renderStep({ bodies, onAdvance });

    await user.click(screen.getByRole("button", { name: "Skip" }));
    expect(onAdvance).toHaveBeenCalledTimes(1);
    expect(bodies["profile"]).toBeUndefined();
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("toasts but still advances when the save fails", async () => {
    const user = userEvent.setup();
    const onAdvance = vi.fn();
    renderStep({ failSave: true, onAdvance });

    await user.click(screen.getByRole("checkbox", { name: ONBOARDING_USE_CASES[0] ?? "" }));
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await waitFor(() => expect(onAdvance).toHaveBeenCalledTimes(1));
    expect(notifyError).toHaveBeenCalledWith(USE_CASE_SAVE_FAILED_TITLE);
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});
