// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Details form (WSET-003): prefill, the six presets, the searchable
// timezone picker, inline name errors with no write, the Updating state,
// the success notice, the silent failed save and the member-dimmed form.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { WorkspaceDetailsForm, DETAILS_SAVE_LABEL, DETAILS_SAVING_LABEL } from "./WorkspaceDetailsForm.js";

vi.mock("../../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";

const DETAILS_PATH = "/api/workspaces/acme/";
const TIMEZONES_PATH = "/api/timezones/";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

const TIMEZONES = {
  timezones: [
    { value: "Pacific/Auckland", label: "Auckland", utc_offset: "UTC+13:00", gmt_offset: "GMT+13:00" },
    { value: "Pacific/Auckland", label: "Wellington", utc_offset: "UTC+13:00", gmt_offset: "GMT+13:00" },
    {
      value: "America/New_York",
      label: "Eastern Time (US and Canada)",
      utc_offset: "UTC-05:00",
      gmt_offset: "GMT-05:00",
    },
  ],
};

interface StubOptions {
  patchStatus?: number;
  holdPatch?: () => Promise<void>;
  timezonesStatus?: number;
  onPatch?: (body: Record<string, unknown>) => void;
}

function stubApi(options: StubOptions = {}) {
  const patches: Array<Record<string, unknown>> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(TIMEZONES_PATH)) {
      if ((options.timezonesStatus ?? 200) !== 200) return jsonResponse(url, 500, {});
      return jsonResponse(url, 200, TIMEZONES);
    }
    if (url.endsWith(DETAILS_PATH) && method === "PATCH") {
      const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      patches.push(body);
      options.onPatch?.(body);
      if (options.holdPatch !== undefined) await options.holdPatch();
      if ((options.patchStatus ?? 200) !== 200) return jsonResponse(url, options.patchStatus ?? 500, {});
      return jsonResponse(url, 200, {
        id: "123e4567-e89b-12d3-a456-426614174000",
        name: body.name ?? "Acme",
        slug: "acme",
        logo: null,
        logo_asset: null,
        logo_url: null,
        owner: "123e4567-e89b-12d3-a456-426614174000",
        organization_size: body.organization_size ?? null,
        timezone: body.timezone ?? "UTC",
        background_color: "#fff",
        total_members: 1,
        created_by: null,
        updated_by: null,
        created_at: "2024-01-02T03:04:05Z",
        updated_at: "2024-02-03T04:05:06Z",
      });
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { patches };
}

function renderForm(
  overrides: Partial<React.ComponentProps<typeof WorkspaceDetailsForm>> = {},
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
) {
  const props: React.ComponentProps<typeof WorkspaceDetailsForm> = {
    workspaceSlug: "acme",
    canManage: true,
    stored: { name: "Acme", organizationSize: null, timezone: "UTC" },
    ...overrides,
  };
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceDetailsForm {...props} />
    </QueryClientProvider>
  );
  return { props, queryClient, view };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("WorkspaceDetailsForm", () => {
  it("prefills the stored details and lists the six size presets", async () => {
    stubApi();
    const user = userEvent.setup();
    renderForm({ stored: { name: "Stored Acme", organizationSize: "11-50", timezone: "UTC" } });
    expect(screen.getByLabelText("Workspace name")).toHaveValue("Stored Acme");
    expect(screen.getByLabelText("Workspace URL")).toHaveValue(`${window.location.origin}/acme`);
    expect(screen.getByRole("button", { name: "11-50" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "11-50" }));
    const list = screen.getByRole("listbox", { name: "Organization size" });
    expect(
      within(list)
        .getAllByRole("option")
        .map((option) => option.textContent)
    ).toEqual(["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"]);
  });

  it("shows the size placeholder when nothing is stored", () => {
    stubApi();
    renderForm();
    expect(screen.getByRole("button", { name: "Select organization size" })).toBeInTheDocument();
  });

  it("saves the name, size and a searched timezone with the Updating state and the success notice", async () => {
    let release: () => void = () => undefined;
    const hold = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { patches } = stubApi({ holdPatch: () => hold });
    const user = userEvent.setup();
    renderForm();

    await user.clear(screen.getByLabelText("Workspace name"));
    await user.type(screen.getByLabelText("Workspace name"), "Parity Workspace");
    await user.click(screen.getByRole("button", { name: "Select organization size" }));
    await user.click(screen.getByRole("option", { name: "11-50" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /UTC/ })).toBeInTheDocument();
    });
    await user.click(screen.getByRole("button", { name: /UTC/ }));
    const search = screen.getByRole("combobox", { name: "Search timezones" });
    await user.type(search, "Auckland");
    await user.click(screen.getByRole("option", { name: /Auckland, Wellington/ }));

    await user.click(screen.getByRole("button", { name: DETAILS_SAVE_LABEL }));
    expect(await screen.findByRole("button", { name: DETAILS_SAVING_LABEL })).toBeInTheDocument();
    release();
    await waitFor(() => {
      expect(notifySuccess).toHaveBeenCalledWith("Success!", "Workspace updated successfully");
    });
    expect(patches).toEqual([{ name: "Parity Workspace", organization_size: "11-50", timezone: "Pacific/Auckland" }]);
    expect(screen.getByLabelText("Workspace name")).toHaveValue("Parity Workspace");
    expect(screen.getByRole("button", { name: "11-50" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Auckland, Wellington/ })).toBeInTheDocument();
  });

  it("rejects an empty name inline with no write", async () => {
    const { patches } = stubApi();
    const user = userEvent.setup();
    renderForm();
    await user.clear(screen.getByLabelText("Workspace name"));
    await user.click(screen.getByRole("button", { name: DETAILS_SAVE_LABEL }));
    expect(screen.getByText("Workspace name is required")).toBeInTheDocument();
    expect(patches).toHaveLength(0);
  });

  it("rejects special characters inline with no write", async () => {
    const { patches } = stubApi();
    const user = userEvent.setup();
    renderForm();
    fireEvent.change(screen.getByLabelText("Workspace name"), { target: { value: "Bad{Name}" } });
    await user.click(screen.getByRole("button", { name: DETAILS_SAVE_LABEL }));
    expect(screen.getByText(/special characters/)).toBeInTheDocument();
    expect(patches).toHaveLength(0);
  });

  it("keeps the typed values with no notice when the save fails", async () => {
    stubApi({ patchStatus: 500 });
    const user = userEvent.setup();
    renderForm();
    await user.clear(screen.getByLabelText("Workspace name"));
    await user.type(screen.getByLabelText("Workspace name"), "Attempt");
    await user.click(screen.getByRole("button", { name: DETAILS_SAVE_LABEL }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: DETAILS_SAVE_LABEL })).toBeEnabled();
    });
    expect(screen.getByLabelText("Workspace name")).toHaveValue("Attempt");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("re-syncs the form when the stored row changes underneath it", async () => {
    stubApi();
    const { view } = renderForm();
    view.rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <WorkspaceDetailsForm
          workspaceSlug="acme"
          canManage
          stored={{ name: "Renamed Acme", organizationSize: "500+", timezone: "UTC" }}
        />
      </QueryClientProvider>
    );
    await waitFor(() => {
      expect(screen.getByLabelText("Workspace name")).toHaveValue("Renamed Acme");
    });
    expect(screen.getByRole("button", { name: "500+" })).toBeInTheDocument();
  });

  it("shows the timezone placeholder when the stored row carries no zone", async () => {
    stubApi();
    renderForm({ stored: { name: "Acme", organizationSize: null, timezone: "" } });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Select a timezone/ })).toBeInTheDocument();
    });
  });

  it("dims the form for members with disabled fields and no save", () => {
    stubApi();
    renderForm({ canManage: false });
    expect(screen.getByTestId("workspace-general-form").className).toContain("opacity-60");
    expect(screen.getByLabelText("Workspace name")).toBeDisabled();
    expect(screen.queryByRole("button", { name: DETAILS_SAVE_LABEL })).not.toBeInTheDocument();
  });

  it("disables the timezone picker and explains a failed timezone read", async () => {
    stubApi({ timezonesStatus: 500 });
    renderForm();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /UTC/ })).toBeDisabled();
    });
  });

  it("ignores submit when a member somehow submits the form", async () => {
    const { patches } = stubApi();
    const { view } = renderForm({ canManage: false });
    const form = view.container.querySelector("form");
    expect(form).not.toBeNull();
    form?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    expect(patches).toHaveLength(0);
  });
});
