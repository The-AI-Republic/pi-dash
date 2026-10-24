// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-system update dialog (EST-014, EST-015): name and values editing
// with creation-grade validation, reorder/add/remove, the reassignment
// confirmation for saved values, and the delete/create/patch save shape.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Estimate, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import {
  EstimateUpdateDialog,
  UPDATE_DIALOG_ADD_LABEL,
  UPDATE_DIALOG_DESCRIPTION,
  UPDATE_DIALOG_NAME_LABEL,
  UPDATE_DIALOG_NAME_REQUIRED,
  UPDATE_DIALOG_REMOVE_LABEL,
  UPDATE_DIALOG_SAVE_LABEL,
  UPDATE_DIALOG_TITLE,
} from "./EstimateUpdateDialog.js";

const WORKSPACE = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const SYSTEM_ID = "44444444-4444-4444-8444-444444444444";
const POINT_A = "aaaaaaaa-0001-4000-8000-000000000001";
const POINT_B = "aaaaaaaa-0002-4000-8000-000000000002";
const POINT_C = "aaaaaaaa-0003-4000-8000-000000000003";
const POINT_D = "aaaaaaaa-0004-4000-8000-000000000004";
const POINT_E = "aaaaaaaa-0005-4000-8000-000000000005";
const POINT_F = "aaaaaaaa-0006-4000-8000-000000000006";

vi.mock("../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
}));

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function point(id: string, key: number, value: string) {
  return { id, key, value, description: "" };
}

function systemFixture(overrides: Partial<Estimate> = {}): Estimate {
  return {
    id: SYSTEM_ID,
    workspace: "11111111-1111-4111-8111-111111111111",
    project: PROJECT,
    name: "Points",
    description: "",
    type: "points",
    last_used: true,
    points: [point(POINT_A, 1, "1"), point(POINT_B, 2, "2"), point(POINT_C, 3, "3")],
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

function stubApi(options: { updateStatus?: number; holdPatch?: Promise<void> } = {}) {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body = init.body === undefined ? undefined : (JSON.parse(init.body) as unknown);
    calls.push({ method, url, body });
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.includes("/estimate-points/") && method === "POST") {
      const input = body as { key: number; value: string };
      return jsonResponse(url, 200, {
        id: `99999999-9999-4999-8999-99999999990${input.key}`,
        key: input.key,
        value: input.value,
        description: "",
      });
    }
    if (url.includes("/estimate-points/") && method === "DELETE") return jsonResponse(url, 200, []);
    if (url.endsWith(`/projects/${PROJECT}/estimates/${SYSTEM_ID}/`) && method === "PATCH") {
      if (options.holdPatch !== undefined) await options.holdPatch;
      if ((options.updateStatus ?? 200) !== 200) return jsonResponse(url, options.updateStatus ?? 500, {});
      const input = body as {
        estimate?: { name?: string };
        estimate_points: { id: string; key: number; value: string }[];
      };
      return jsonResponse(url, 200, {
        ...systemFixture(),
        name: input.estimate?.name ?? "Points",
        points: input.estimate_points.map((row) => ({ ...row, description: "" })),
      });
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { calls };
}

function renderDialog(system: Estimate = systemFixture(), onClose = vi.fn()) {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <EstimateUpdateDialog workspaceSlug={WORKSPACE} projectId={PROJECT} system={system} onClose={onClose} />
    </QueryClientProvider>
  );
  return { onClose };
}

function dialog(): HTMLElement {
  return screen.getByRole("dialog");
}

function values(): string[] {
  return within(dialog())
    .getAllByTestId("estimate-edit-value")
    .map((row) => row.textContent ?? "");
}

async function editValue(
  user: ReturnType<typeof userEvent.setup>,
  index: number,
  next: string,
  commit: "enter" | "button" | "escape" = "enter"
): Promise<void> {
  await user.click(within(dialog()).getByRole("button", { name: `Edit value ${index + 1}` }));
  const input = within(dialog()).getByRole("textbox", { name: `Value ${index + 1}` });
  await user.clear(input);
  if (next !== "") await user.type(input, next);
  if (commit === "enter") await user.keyboard("{Enter}");
  else if (commit === "button") await user.click(within(dialog()).getByRole("button", { name: "Save" }));
  else await user.keyboard("{Escape}");
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("EstimateUpdateDialog rendering", () => {
  it("shows the name and values in key order with the family label", () => {
    stubApi();
    renderDialog(
      systemFixture({
        points: [point(POINT_C, 3, "3"), point(POINT_A, 1, "1"), point(POINT_B, 2, "2")],
      })
    );
    expect(within(dialog()).getByText(UPDATE_DIALOG_TITLE)).toBeInTheDocument();
    expect(within(dialog()).getByText(UPDATE_DIALOG_DESCRIPTION)).toBeInTheDocument();
    expect(within(dialog()).getByRole("textbox", { name: UPDATE_DIALOG_NAME_LABEL })).toHaveValue("Points");
    expect(within(dialog()).getByText("Points")).toBeInTheDocument();
    expect(values()).toEqual(["1", "2", "3"]);
  });

  it("humanizes time values and edits the raw minutes", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog(
      systemFixture({
        name: "Time",
        type: "time",
        points: [point(POINT_A, 1, "90"), point(POINT_B, 2, "150")],
      })
    );
    expect(values()).toEqual(["1h 30m", "2h 30m"]);
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 1" }));
    expect(within(dialog()).getByRole("textbox", { name: "Value 1" })).toHaveValue("90");
  });

  it("reads an unknown stored type as text", () => {
    stubApi();
    renderDialog(systemFixture({ type: "forthcoming" }));
    expect(within(dialog()).getByText("Categories")).toBeInTheDocument();
    // Numeric stored values flag the categories rule on every row.
    expect(within(dialog()).getAllByText(/character/i)).toHaveLength(3);
  });

  it("hides remove at two values and add at six", async () => {
    stubApi();
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <EstimateUpdateDialog
          workspaceSlug={WORKSPACE}
          projectId={PROJECT}
          system={systemFixture({ points: [point(POINT_A, 1, "1"), point(POINT_B, 2, "2")] })}
          onClose={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(values()).toEqual(["1", "2"]);
    expect(within(dialog()).queryByRole("button", { name: "Remove value 1" })).not.toBeInTheDocument();
    expect(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_ADD_LABEL })).toBeInTheDocument();
    unmount();
    renderDialog(
      systemFixture({
        points: [
          point(POINT_A, 1, "1"),
          point(POINT_B, 2, "2"),
          point(POINT_C, 3, "3"),
          point(POINT_D, 4, "4"),
          point(POINT_E, 5, "5"),
          point(POINT_F, 6, "6"),
        ],
      })
    );
    expect(within(dialog()).queryByRole("button", { name: UPDATE_DIALOG_ADD_LABEL })).not.toBeInTheDocument();
    expect(within(dialog()).getByRole("button", { name: "Remove value 1" })).toBeInTheDocument();
  });
});

describe("EstimateUpdateDialog inline editing", () => {
  it("commits an edit and closes an unchanged one quietly", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await editValue(user, 0, "9");
    expect(values()).toEqual(["9", "2", "3"]);
    expect(within(dialog()).queryByRole("textbox", { name: "Value 1" })).not.toBeInTheDocument();
    await editValue(user, 0, "9");
    expect(values()).toEqual(["9", "2", "3"]);
    expect(within(dialog()).queryByText(/already exists|empty|numeric/i)).not.toBeInTheDocument();
  });

  it("opens the editor on double-click", async () => {
    stubApi();
    renderDialog();
    fireEvent.doubleClick(within(dialog()).getAllByTestId("estimate-edit-value")[1] as HTMLElement);
    expect(within(dialog()).getByRole("textbox", { name: "Value 2" })).toHaveValue("2");
  });

  it("refuses an empty draft and keeps the editor open", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await editValue(user, 0, "");
    expect(within(dialog()).getByRole("textbox", { name: "Value 1" })).toBeInTheDocument();
    expect(within(dialog()).getByText(/empty/i)).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(values()).toEqual(["1", "2", "3"]);
  });

  it("refuses an invalid draft with its explanation", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await editValue(user, 1, "abc", "button");
    expect(within(dialog()).getByRole("textbox", { name: "Value 2" })).toBeInTheDocument();
    expect(within(dialog()).getByText(/numeric/i)).toBeInTheDocument();
    const editor = within(dialog()).getByRole("textbox", { name: "Value 2" }).closest("form") as HTMLElement;
    await user.click(within(editor).getByRole("button", { name: "Cancel" }));
    expect(values()).toEqual(["1", "2", "3"]);
  });

  it("discards the draft on Escape without touching the saved row", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await editValue(user, 0, "9", "escape");
    expect(within(dialog()).queryByRole("textbox", { name: "Value 1" })).not.toBeInTheDocument();
    expect(values()).toEqual(["1", "2", "3"]);
    expect(within(dialog()).getByText(UPDATE_DIALOG_TITLE)).toBeInTheDocument();
  });

  it("drops a blank add placeholder on cancel and keeps a committed new row", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_ADD_LABEL }));
    expect(within(dialog()).getByRole("textbox", { name: "Value 4" })).toBeInTheDocument();
    const editor = within(dialog()).getByRole("textbox", { name: "Value 4" }).closest("form") as HTMLElement;
    await user.click(within(editor).getByRole("button", { name: "Cancel" }));
    expect(values()).toEqual(["1", "2", "3"]);

    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_ADD_LABEL }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 4" });
    await user.type(input, "4");
    await user.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(values()).toEqual(["1", "2", "3", "4"]);
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 4" }));
    await user.keyboard("{Escape}");
    expect(values()).toEqual(["1", "2", "3", "4"]);
  });

  it("caps typed values at 20 characters", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog(systemFixture({ type: "categories", points: [point(POINT_A, 1, "Easy")] }));
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 1" }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 1" });
    await user.clear(input);
    await user.type(input, "x".repeat(25));
    expect(input).toHaveValue("x".repeat(20));
  });
});

describe("EstimateUpdateDialog reordering", () => {
  it("drags rows to reorder with positions renumbered", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    const handles = within(dialog()).getAllByRole("button", { name: /^Reorder value/ });
    fireEvent.drop(within(dialog()).getAllByTestId("estimate-edit-value")[0] as HTMLElement);
    expect(values()).toEqual(["1", "2", "3"]);
    fireEvent.dragStart(handles[0] as HTMLElement);
    fireEvent.drop(within(dialog()).getAllByTestId("estimate-edit-value")[0] as HTMLElement);
    expect(values()).toEqual(["1", "2", "3"]);
    fireEvent.dragStart(handles[0] as HTMLElement);
    const target = within(dialog()).getAllByTestId("estimate-edit-value")[2] as HTMLElement;
    fireEvent.dragOver(target);
    fireEvent.drop(target);
    expect(values()).toEqual(["2", "1", "3"]);
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      estimate: { name: "Points" },
      estimate_points: [
        { id: POINT_B, key: 1, value: "2" },
        { id: POINT_A, key: 2, value: "1" },
        { id: POINT_C, key: 3, value: "3" },
      ],
    });
  });

  it("places a dragged row in the final slot when dropped on the lower half", () => {
    stubApi();
    renderDialog(
      systemFixture({
        points: [
          point(POINT_A, 1, "1"),
          point(POINT_B, 2, "2"),
          point(POINT_C, 3, "3"),
          point(POINT_D, 4, "4"),
          point(POINT_E, 5, "5"),
          point(POINT_F, 6, "6"),
        ],
      })
    );
    const handles = within(dialog()).getAllByRole("button", { name: /^Reorder value/ });
    const targetValue = within(dialog()).getAllByTestId("estimate-edit-value")[5] as HTMLElement;
    const targetRow = targetValue.closest("li") as HTMLElement;
    vi.spyOn(targetRow, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 100,
      width: 200,
      height: 40,
      top: 100,
      right: 200,
      bottom: 140,
      left: 0,
      toJSON: () => ({}),
    });
    fireEvent.dragStart(handles[0] as HTMLElement);
    fireEvent(targetRow, new MouseEvent("drop", { bubbles: true, cancelable: true, clientY: 135 }));
    expect(values()).toEqual(["2", "3", "4", "5", "6", "1"]);
  });

  it("reorders with keyboard move controls and disables the edges", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    expect(within(dialog()).getByRole("button", { name: "Move value 1 up" })).toBeDisabled();
    expect(within(dialog()).getByRole("button", { name: "Move value 3 down" })).toBeDisabled();
    await user.click(within(dialog()).getByRole("button", { name: "Move value 2 up" }));
    expect(values()).toEqual(["2", "1", "3"]);
    await user.click(within(dialog()).getByRole("button", { name: "Move value 1 down" }));
    expect(values()).toEqual(["1", "2", "3"]);
  });
});

describe("EstimateUpdateDialog value removal", () => {
  it("drops a new row without a confirmation", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_ADD_LABEL }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 4" });
    await user.type(input, "4");
    await user.click(within(dialog()).getByRole("button", { name: "Save" }));
    await user.click(within(dialog()).getByRole("button", { name: "Remove value 4" }));
    expect(values()).toEqual(["1", "2", "3"]);
    expect(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL })).toBeInTheDocument();
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(calls.filter((call) => call.method === "DELETE")).toHaveLength(0);
    expect(calls.filter((call) => call.method === "POST")).toHaveLength(0);
  });

  it("asks before removing a saved value, and cancel keeps the row", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: "Remove value 2" }));
    expect(within(dialog()).getByText("Remove 2?")).toBeInTheDocument();
    expect(
      within(dialog()).getByRole("radio", { name: "Move affected work items to another value" })
    ).toBeInTheDocument();
    const clear = within(dialog()).getByRole("radio", { name: "Clear the estimate on affected work items" });
    expect(clear).toBeChecked();
    expect(within(dialog()).getByLabelText("Replacement value")).toBeDisabled();
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(values()).toEqual(["1", "2", "3"]);
    expect(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL })).toBeInTheDocument();
    expect(calls).toHaveLength(0);
  });

  it("confirms a removal with a replacement and saves the move", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_ADD_LABEL }));
    await user.type(within(dialog()).getByRole("textbox", { name: "Value 4" }), "4");
    await user.click(within(dialog()).getByRole("button", { name: "Save" }));
    await user.click(within(dialog()).getByRole("button", { name: "Remove value 2" }));
    await user.click(within(dialog()).getByRole("radio", { name: "Move affected work items to another value" }));
    const replacement = within(dialog()).getByLabelText("Replacement value");
    expect(replacement).toBeEnabled();
    expect(replacement).toHaveValue(POINT_A);
    await user.selectOptions(replacement, POINT_C);
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_REMOVE_LABEL }));
    expect(values()).toEqual(["1", "3", "4"]);
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    const deletes = calls.filter((call) => call.method === "DELETE");
    expect(deletes).toHaveLength(1);
    expect(deletes[0]?.body).toEqual({ new_estimate_id: POINT_C });
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      estimate: { name: "Points" },
      estimate_points: [
        { id: POINT_A, key: 1, value: "1" },
        { id: POINT_C, key: 2, value: "3" },
        { id: "99999999-9999-4999-8999-999999999903", key: 3, value: "4" },
      ],
    });
  });

  it("confirms a removal with clearing and sends no replacement", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: "Remove value 1" }));
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_REMOVE_LABEL }));
    expect(values()).toEqual(["2", "3"]);
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    const deletes = calls.filter((call) => call.method === "DELETE");
    expect(deletes).toHaveLength(1);
    expect(deletes[0]?.body).toBeUndefined();
  });

  it("offers only clearing when no other saved value remains", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    for (const label of ["Value 4", "Value 5"]) {
      await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_ADD_LABEL }));
      await user.type(within(dialog()).getByRole("textbox", { name: label }), label === "Value 4" ? "4" : "5");
      await user.click(within(dialog()).getByRole("button", { name: "Save" }));
    }
    for (const name of ["Remove value 2", "Remove value 2"]) {
      await user.click(within(dialog()).getByRole("button", { name }));
      await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_REMOVE_LABEL }));
    }
    expect(values()).toEqual(["1", "4", "5"]);
    await user.click(within(dialog()).getByRole("button", { name: "Remove value 1" }));
    expect(
      within(dialog()).queryByRole("radio", { name: "Move affected work items to another value" })
    ).not.toBeInTheDocument();
    expect(within(dialog()).getByText(/clearing is the only choice/i)).toBeInTheDocument();
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_REMOVE_LABEL }));
    expect(values()).toEqual(["4", "5"]);
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(calls.filter((call) => call.method === "DELETE")).toHaveLength(3);
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      estimate: { name: "Points" },
      estimate_points: [
        { id: "99999999-9999-4999-8999-999999999901", key: 1, value: "4" },
        { id: "99999999-9999-4999-8999-999999999902", key: 2, value: "5" },
      ],
    });
  });
});

describe("EstimateUpdateDialog save and dismissal", () => {
  it("saves a rename with the full id set and closes", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    const { onClose } = renderDialog();
    await user.clear(within(dialog()).getByRole("textbox", { name: UPDATE_DIALOG_NAME_LABEL }));
    await user.type(within(dialog()).getByRole("textbox", { name: UPDATE_DIALOG_NAME_LABEL }), "Renamed");
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      estimate: { name: "Renamed" },
      estimate_points: [
        { id: POINT_A, key: 1, value: "1" },
        { id: POINT_B, key: 2, value: "2" },
        { id: POINT_C, key: 3, value: "3" },
      ],
    });
    expect(onClose).toHaveBeenCalled();
  });

  it("folds a valid pending draft into the save", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 1" }));
    await user.type(within(dialog()).getByRole("textbox", { name: "Value 1" }), "0");
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      estimate: { name: "Points" },
      estimate_points: [
        { id: POINT_A, key: 1, value: "10" },
        { id: POINT_B, key: 2, value: "2" },
        { id: POINT_C, key: 3, value: "3" },
      ],
    });
  });

  it("keeps an invalid pending draft and blocks the write", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 1" }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 1" });
    await user.clear(input);
    await user.type(input, "abc");
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(within(dialog()).getByText(/numeric/i)).toBeInTheDocument());
    expect(within(dialog()).getByRole("textbox", { name: "Value 1" })).toBeInTheDocument();
    expect(calls).toHaveLength(0);
  });

  it("refuses an empty name and clears the complaint while typing", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    const name = within(dialog()).getByRole("textbox", { name: UPDATE_DIALOG_NAME_LABEL });
    await user.clear(name);
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    expect(within(dialog()).getByText(UPDATE_DIALOG_NAME_REQUIRED)).toBeInTheDocument();
    expect(calls).toHaveLength(0);
    await user.type(name, "Rescued");
    expect(within(dialog()).queryByText(UPDATE_DIALOG_NAME_REQUIRED)).not.toBeInTheDocument();
  });

  it("blocks the save while a blank row lingers", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_ADD_LABEL }));
    const handles = within(dialog()).getAllByRole("button", { name: /^Reorder value/ });
    fireEvent.dragStart(handles[0] as HTMLElement);
    fireEvent.drop(within(dialog()).getAllByTestId("estimate-edit-value")[1] as HTMLElement);
    expect(within(dialog()).queryByRole("textbox", { name: "Value 4" })).not.toBeInTheDocument();
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(within(dialog()).getByText(/empty/i)).toBeInTheDocument());
    expect(calls).toHaveLength(0);
  });

  it("reports a failed save and stays open with its content", async () => {
    const user = userEvent.setup();
    stubApi({ updateStatus: 500 });
    renderDialog();
    await user.clear(within(dialog()).getByRole("textbox", { name: UPDATE_DIALOG_NAME_LABEL }));
    await user.type(within(dialog()).getByRole("textbox", { name: UPDATE_DIALOG_NAME_LABEL }), "Renamed");
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(within(dialog()).getByRole("textbox", { name: UPDATE_DIALOG_NAME_LABEL })).toHaveValue("Renamed");
    expect(values()).toEqual(["1", "2", "3"]);
  });

  it("blocks dismissal while the save is in flight", async () => {
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubApi({ holdPatch: held });
    const user = userEvent.setup();
    const { onClose } = renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(within(dialog()).getByRole("button", { name: "Cancel" })).toBeDisabled());
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    fireEvent.doubleClick(within(dialog()).getAllByTestId("estimate-edit-value")[0] as HTMLElement);
    expect(within(dialog()).queryByRole("textbox", { name: "Value 1" })).not.toBeInTheDocument();
    release();
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("cancel and the close control discard without a write", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    const { onClose } = renderDialog();
    await user.type(within(dialog()).getByRole("textbox", { name: UPDATE_DIALOG_NAME_LABEL }), " (draft)");
    await user.click(within(dialog()).getByRole("button", { name: "Close dialog" }));
    expect(onClose).toHaveBeenCalled();
    expect(calls).toHaveLength(0);
    onClose.mockClear();
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(calls).toHaveLength(0);
  });
});
