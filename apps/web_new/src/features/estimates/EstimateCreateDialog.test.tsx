// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-estimate-system dialog (PSET-049, EST-008–013): step counter, back
// resetting the draft, cancel without a write, family switching, value
// add/edit/remove guards, inline validation and submit.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { EstimateCreateDialog, ESTIMATE_DIALOG_TITLE } from "./EstimateCreateDialog.js";
import { valueSetProblems } from "./model.js";

const WORKSPACE = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

vi.mock("../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
}));

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

interface Posted {
  estimate: { name: string; type: string; last_used: boolean };
  estimate_points: { key: number; value: string }[];
}

function stubApi(options: { status?: number } = {}) {
  const posts: Posted[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/${WORKSPACE}/projects/${PROJECT}/estimates/`) && method === "POST") {
      const body = JSON.parse(init.body ?? "{}") as Posted;
      posts.push(body);
      if ((options.status ?? 200) !== 200) return jsonResponse(url, options.status ?? 500, {});
      return jsonResponse(url, 200, {
        id: "99999999-9999-4999-8999-999999999999",
        workspace: "11111111-1111-4111-8111-111111111111",
        project: PROJECT,
        name: body.estimate.name,
        description: "",
        type: body.estimate.type,
        last_used: body.estimate.last_used,
        points: [],
        created_at: "2024-01-01T00:00:00Z",
        updated_at: "2024-01-01T00:00:00Z",
        created_by: null,
        updated_by: null,
      });
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { posts };
}

function renderDialog(onOpenChange = vi.fn(), onCreated = vi.fn()) {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <EstimateCreateDialog
        workspaceSlug={WORKSPACE}
        projectId={PROJECT}
        open
        onOpenChange={onOpenChange}
        onCreated={onCreated}
      />
    </QueryClientProvider>
  );
  return { onOpenChange, onCreated };
}

function dialog(): HTMLElement {
  return screen.getByRole("dialog");
}

async function pickStarter(user: ReturnType<typeof userEvent.setup>, title: string): Promise<void> {
  await user.click(within(dialog()).getByRole("button", { name: new RegExp(title, "i") }));
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("EstimateCreateDialog", () => {
  it("opens on step one and moves to step two with a starter's values", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    expect(within(dialog()).getByText("Step 1 of 2")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /back/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create estimate" })).not.toBeInTheDocument();

    await pickStarter(user, "Fibonacci");
    expect(within(dialog()).getByText("Step 2 of 2")).toBeInTheDocument();
    expect(
      within(dialog())
        .getAllByTestId("estimate-value")
        .map((row) => row.textContent)
    ).toEqual(["1", "2", "3", "5", "8", "13"]);
  });

  it("back returns to step one and drops the draft", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 1" }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 1" });
    await user.clear(input);
    await user.type(input, "9{Enter}");
    expect(within(dialog()).getByText("9")).toBeInTheDocument();
    await user.click(within(dialog()).getByRole("button", { name: /back/i }));
    expect(within(dialog()).getByText("Step 1 of 2")).toBeInTheDocument();
    await pickStarter(user, "Custom");
    expect(
      within(dialog())
        .getAllByTestId("estimate-value")
        .map((row) => row.textContent)
    ).toEqual(["1", "2"]);
  });

  it("cancel closes on step one without a write", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    const { onOpenChange } = renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(posts).toHaveLength(0);
  });

  it("cancel closes on step two without a write", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    const { onOpenChange } = renderDialog();
    await pickStarter(user, "Fibonacci");
    expect(within(dialog()).getByText("Step 2 of 2")).toBeInTheDocument();
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(posts).toHaveLength(0);
  });

  it("closes through the dialog's own close control", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: "Close dialog" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("switches families and offers each family's starters", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    expect(within(dialog()).getByRole("button", { name: /fibonacci/i })).toBeInTheDocument();
    expect(within(dialog()).queryByRole("radio", { name: /time/i })).not.toBeInTheDocument();
    await user.click(within(dialog()).getByRole("radio", { name: "Categories" }));
    expect(within(dialog()).getByRole("button", { name: /t-shirt sizes/i })).toBeInTheDocument();
    expect(within(dialog()).queryByRole("button", { name: /fibonacci/i })).not.toBeInTheDocument();
    await pickStarter(user, "T-Shirt Sizes");
    expect(
      within(dialog())
        .getAllByTestId("estimate-value")
        .map((row) => row.textContent)
    ).toEqual(["XS", "S", "M", "L", "XL", "XXL"]);
  });

  it("adds a value up to six and removes down to two", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    expect(within(dialog()).getByRole("button", { name: "Remove value 1" })).toBeDisabled();
    await user.click(within(dialog()).getByRole("button", { name: "Add a value" }));
    expect(within(dialog()).getAllByTestId("estimate-value")).toHaveLength(3);
    expect(within(dialog()).getByRole("button", { name: "Remove value 1" })).toBeEnabled();
    await user.click(within(dialog()).getByRole("button", { name: "Remove value 3" }));
    expect(
      within(dialog())
        .getAllByTestId("estimate-value")
        .map((row) => row.textContent)
    ).toEqual(["1", "2"]);
    for (let added = 0; added < 4; added += 1) {
      await user.click(within(dialog()).getByRole("button", { name: "Add a value" }));
    }
    expect(within(dialog()).getAllByTestId("estimate-value")).toHaveLength(6);
    expect(within(dialog()).getByRole("button", { name: "Add a value" })).toBeDisabled();
  });

  it("blocks submit on problems and shows the inline markers", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 1" }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 1" });
    await user.clear(input);
    await user.type(input, "2{Enter}");
    expect(valueSetProblems("points", ["2", "2"])).toHaveLength(1);
    await user.click(within(dialog()).getByRole("button", { name: "Create estimate" }));
    expect(within(dialog()).getByText("Each value must be different.")).toBeInTheDocument();
    expect(posts).toHaveLength(0);
  });

  it("submits the trimmed values under the family name and closes", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    const { onCreated, onOpenChange } = renderDialog();
    await pickStarter(user, "Linear");
    await user.click(within(dialog()).getByRole("button", { name: "Create estimate" }));
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(posts).toEqual([
      {
        estimate: { name: "Points", type: "points", last_used: true },
        estimate_points: [
          { key: 1, value: "1" },
          { key: 2, value: "2" },
          { key: 3, value: "3" },
          { key: 4, value: "4" },
          { key: 5, value: "5" },
          { key: 6, value: "6" },
        ],
      },
    ]);
  });

  it("clears a row after typing and records the edit", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    // A second click on the row is inert; only the edit control opens it.
    await user.click(within(dialog()).getByText("1", { selector: "[data-testid='estimate-value']" }));
    expect(within(dialog()).queryByRole("textbox")).not.toBeInTheDocument();
    const row = within(dialog()).getByText("2", { selector: "[data-testid='estimate-value']" });
    await user.dblClick(row);
    const input = within(dialog()).getByRole("textbox", { name: "Value 2" });
    await user.type(input, "0");
    await user.tab();
    expect(
      within(dialog())
        .getAllByTestId("estimate-value")
        .map((cell) => cell.textContent)
    ).toEqual(["1", "20"]);
  });

  it("caps a typed value and submits the capped text", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("radio", { name: "Categories" }));
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: "Add a value" }));
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 3" }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 3" });
    await user.type(input, "x".repeat(60));
    // The input caps what it accepts at the server's length limit.
    expect((input as HTMLInputElement).value).toHaveLength(40);
    await user.click(within(dialog()).getByRole("button", { name: "Create estimate" }));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]?.estimate.name).toBe("Categories");
    expect(posts[0]?.estimate_points.at(-1)).toEqual({ key: 3, value: "x".repeat(40) });
  });

  it("keeps the dialog open with the draft when the server refuses", async () => {
    const user = userEvent.setup();
    stubApi({ status: 500 });
    const { onCreated, onOpenChange } = renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: "Create estimate" }));
    await waitFor(() => expect(within(dialog()).getByRole("button", { name: "Create estimate" })).toBeEnabled());
    expect(onCreated).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(dialog()).toHaveTextContent(ESTIMATE_DIALOG_TITLE);
  });
});
