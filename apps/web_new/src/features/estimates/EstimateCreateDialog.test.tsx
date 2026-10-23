// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-estimate-system dialog (EST-008–013): two steps with a counter,
// back/cancel/reset, edition-gated families and templates, drag and
// keyboard reordering, inline validation and submit success/failure.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { ESTIMATE_TIME_FAMILY_FLAG } from "../../core/edition/types.js";
import {
  EstimateCreateDialog,
  ESTIMATE_ADD_LABEL,
  ESTIMATE_DIALOG_TITLE,
  ESTIMATE_SUBMIT_LABEL,
} from "./EstimateCreateDialog.js";

const WORKSPACE = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

const notify = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));
vi.mock("../../shared/shell/index.js", () => ({
  notifyError: notify.error,
  notifySuccess: notify.success,
  notifyInfo: vi.fn(),
}));

const editionHarness = vi.hoisted(() => ({ flags: {} as Record<string, boolean> }));
vi.mock("../../core/edition/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../core/edition/index.js")>();
  return {
    ...original,
    get edition() {
      return { ...original.edition, flags: editionHarness.flags };
    },
  };
});

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

interface Posted {
  estimate: { name: string; type: string; last_used: boolean };
  estimate_points: { key: number; value: string }[];
}

function stubApi(options: { status?: number; hold?: boolean } = {}) {
  const posts: Posted[] = [];
  const requests: string[] = [];
  let release: () => void = () => undefined;
  const gate = options.hold === true ? new Promise<void>((resolve) => (release = resolve)) : Promise.resolve();
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    requests.push(`${method} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/${WORKSPACE}/projects/${PROJECT}/estimates/`) && method === "POST") {
      const body = JSON.parse(init.body ?? "{}") as Posted;
      posts.push(body);
      await gate;
      if ((options.status ?? 200) !== 200) return jsonResponse(url, options.status ?? 500, {});
      return jsonResponse(url, 200, {
        id: "99999999-9999-4999-8999-999999999999",
        workspace: "11111111-1111-4111-8111-111111111111",
        project: PROJECT,
        name: body.estimate.name,
        description: "",
        type: body.estimate.type,
        last_used: body.estimate.last_used,
        points: body.estimate_points.map((point) => ({
          ...point,
          id: `00000000-0000-4000-8000-00000000000${point.key}`,
          description: "",
        })),
        created_at: "2024-01-01T00:00:00Z",
        updated_at: "2024-01-01T00:00:00Z",
        created_by: null,
        updated_by: null,
      });
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { posts, requests, release: () => release() };
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

function renderClosableDialog(onOpenChange = vi.fn()): void {
  function Harness(): React.ReactElement {
    const [open, setOpen] = React.useState(true);
    return (
      <>
        <EstimateCreateDialog
          workspaceSlug={WORKSPACE}
          projectId={PROJECT}
          open={open}
          onOpenChange={(next) => {
            setOpen(next);
            onOpenChange(next);
          }}
        />
        <button type="button" onClick={() => setOpen((current) => !current)}>
          Toggle estimate dialog
        </button>
      </>
    );
  }
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Harness />
    </QueryClientProvider>
  );
}

function values(): string[] {
  return within(dialog())
    .getAllByTestId("estimate-value")
    .map((row) => row.textContent ?? "");
}

async function pickStarter(user: ReturnType<typeof userEvent.setup>, title: string): Promise<void> {
  await user.click(within(dialog()).getByRole("button", { name: new RegExp(title, "i") }));
}

async function editValue(
  user: ReturnType<typeof userEvent.setup>,
  index: number,
  next: string,
  commit: "enter" | "button" | "blur" | "escape" = "enter"
): Promise<void> {
  await user.click(within(dialog()).getByRole("button", { name: `Edit value ${index + 1}` }));
  const input = within(dialog()).getByRole("textbox", { name: `Value ${index + 1}` });
  await user.clear(input);
  if (next !== "") await user.type(input, next);
  if (commit === "enter") await user.keyboard("{Enter}");
  else if (commit === "button") await user.click(within(dialog()).getByRole("button", { name: "Save" }));
  else if (commit === "blur") await user.tab();
  else await user.keyboard("{Escape}");
}

beforeEach(() => {
  editionHarness.flags = {};
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("EstimateCreateDialog steps and dismissal", () => {
  it("opens on step one with a counter and no back/submit controls", async () => {
    stubApi();
    renderDialog();
    expect(within(dialog()).getByText("Step 1 of 2")).toBeInTheDocument();
    expect(within(dialog()).queryByRole("button", { name: /back/i })).not.toBeInTheDocument();
    expect(within(dialog()).queryByRole("button", { name: ESTIMATE_SUBMIT_LABEL })).not.toBeInTheDocument();
  });

  it("picking a starter moves to step two with its values and offers back", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Fibonacci");
    expect(within(dialog()).getByText("Step 2 of 2")).toBeInTheDocument();
    expect(within(dialog()).getByRole("button", { name: /back/i })).toBeInTheDocument();
    expect(values()).toEqual(["1", "2", "3", "5", "8", "13"]);
  });

  it("back returns to step one and drops the draft", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await editValue(user, 0, "9");
    expect(values()).toEqual(["9", "2"]);
    await user.click(within(dialog()).getByRole("button", { name: /back/i }));
    expect(within(dialog()).getByText("Step 1 of 2")).toBeInTheDocument();
    expect(within(dialog()).getByRole("radio", { name: "Points" })).toBeChecked();
    await pickStarter(user, "Custom");
    expect(values()).toEqual(["1", "2"]);
  });

  it("cancel closes on step one and two without a write", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    const { onOpenChange } = renderDialog();
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(posts).toHaveLength(0);
    onOpenChange.mockClear();
    await pickStarter(user, "Fibonacci");
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

  it("reopening resets the step, family and values", async () => {
    const user = userEvent.setup();
    stubApi();
    renderClosableDialog();
    await user.click(within(dialog()).getByRole("radio", { name: "Categories" }));
    await pickStarter(user, "Custom");
    await editValue(user, 0, "99");
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Toggle estimate dialog" }));
    expect(within(dialog()).getByText("Step 1 of 2")).toBeInTheDocument();
    expect(within(dialog()).getByRole("radio", { name: "Points" })).toBeChecked();
    await pickStarter(user, "Custom");
    expect(values()).toEqual(["1", "2"]);
  });
});

describe("EstimateCreateDialog families and templates", () => {
  it("renders only points and categories in OSS, with points selected", async () => {
    stubApi();
    renderDialog();
    const radios = within(dialog()).getAllByRole("radio");
    expect(radios.map((radio) => (radio as HTMLInputElement).value)).toEqual(["points", "categories"]);
    expect(within(dialog()).queryByRole("radio", { name: /time/i })).not.toBeInTheDocument();
    expect(within(dialog()).getByRole("radio", { name: "Points" })).toBeChecked();
  });

  it("adds time when the cloud capability is on", async () => {
    const user = userEvent.setup();
    editionHarness.flags = { [ESTIMATE_TIME_FAMILY_FLAG]: true };
    stubApi();
    renderDialog();
    expect(
      within(dialog())
        .getAllByRole("radio")
        .map((radio) => (radio as HTMLInputElement).value)
    ).toEqual(["points", "categories", "time"]);
    await user.click(within(dialog()).getByRole("radio", { name: "Time" }));
    expect(within(dialog()).getByRole("button", { name: /hours/i })).toBeInTheDocument();
    expect(within(dialog()).getByRole("button", { name: /custom/i })).toHaveTextContent("1m, 2m");
  });

  it("switches families and replaces the starter cards", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    expect(within(dialog()).getByRole("button", { name: /fibonacci/i })).toBeInTheDocument();
    await user.click(within(dialog()).getByRole("radio", { name: "Categories" }));
    expect(within(dialog()).getByRole("button", { name: /t-shirt sizes/i })).toBeInTheDocument();
    expect(within(dialog()).getByRole("button", { name: /easy to hard/i })).toBeInTheDocument();
    expect(within(dialog()).queryByRole("button", { name: /fibonacci/i })).not.toBeInTheDocument();
    await pickStarter(user, "T-Shirt Sizes");
    expect(values()).toEqual(["XS", "S", "M", "L", "XL", "XXL"]);
  });

  it("refuses a family and submit when the edition drops the capability", async () => {
    const user = userEvent.setup();
    editionHarness.flags = { [ESTIMATE_TIME_FAMILY_FLAG]: true };
    const { posts } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("radio", { name: "Time" }));
    await pickStarter(user, "Custom");
    editionHarness.flags = {};
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    expect(posts).toHaveLength(0);
    expect(values()).toEqual(["1m", "2m"]);
    await user.click(within(dialog()).getByRole("button", { name: /back/i }));
    await user.click(within(dialog()).getByRole("button", { name: /hours/i }));
    expect(within(dialog()).getByText("Step 1 of 2")).toBeInTheDocument();
  });

  it("loads the category and time templates and the scratch cards", async () => {
    const user = userEvent.setup();
    editionHarness.flags = { [ESTIMATE_TIME_FAMILY_FLAG]: true };
    stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("radio", { name: "Categories" }));
    await pickStarter(user, "Easy to hard");
    expect(values()).toEqual(["Easy", "Medium", "Hard", "Very Hard"]);
    await user.click(within(dialog()).getByRole("button", { name: /back/i }));
    await user.click(within(dialog()).getByRole("radio", { name: "Time" }));
    await pickStarter(user, "Hours");
    expect(values()).toEqual(["1m", "2m", "3m", "4m", "5m", "6m"]);
    await user.click(within(dialog()).getByRole("button", { name: /back/i }));
    await user.click(within(dialog()).getByRole("radio", { name: "Time" }));
    await pickStarter(user, "Custom");
    expect(values()).toEqual(["1m", "2m"]);
  });

  it("browsing starters and canceling stores nothing", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    const { onOpenChange } = renderDialog();
    await pickStarter(user, "Squares");
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(posts).toHaveLength(0);
  });
});

describe("EstimateCreateDialog value arrangement", () => {
  it("reorders by drag and persists renumbered positions", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_ADD_LABEL }));
    const addInput = within(dialog()).getByRole("textbox", { name: "Value 3" });
    await user.type(addInput, "3{Enter}");
    expect(values()).toEqual(["1", "2", "3"]);
    const handles = within(dialog()).getAllByRole("button", { name: /^Reorder value/ });
    fireEvent.dragStart(handles[0] as HTMLElement);
    fireEvent.drop(within(dialog()).getAllByTestId("estimate-value")[0] as HTMLElement);
    expect(values()).toEqual(["1", "2", "3"]);
    fireEvent.dragStart(handles[0] as HTMLElement);
    fireEvent.dragOver(within(dialog()).getAllByTestId("estimate-value")[1] as HTMLElement);
    fireEvent.dragOver(within(dialog()).getAllByTestId("estimate-value")[2] as HTMLElement);
    fireEvent.drop(within(dialog()).getAllByTestId("estimate-value")[2] as HTMLElement);
    expect(values()).toEqual(["2", "1", "3"]);
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]?.estimate_points).toEqual([
      { key: 1, value: "2" },
      { key: 2, value: "1" },
      { key: 3, value: "3" },
    ]);
  });

  it("reorders with keyboard move controls and disables the edges", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    expect(within(dialog()).getByRole("button", { name: "Move value 1 up" })).toBeDisabled();
    expect(within(dialog()).getByRole("button", { name: "Move value 2 down" })).toBeDisabled();
    await user.click(within(dialog()).getByRole("button", { name: "Move value 2 up" }));
    expect(values()).toEqual(["2", "1"]);
    await user.click(within(dialog()).getByRole("button", { name: "Move value 1 down" }));
    expect(values()).toEqual(["1", "2"]);
  });

  it("hides remove at two and add at six; remove renumbers", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    expect(within(dialog()).queryByRole("button", { name: "Remove value 1" })).not.toBeInTheDocument();
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_ADD_LABEL }));
    await user.type(within(dialog()).getByRole("textbox", { name: "Value 3" }), "3{Enter}");
    expect(within(dialog()).getByRole("button", { name: "Remove value 1" })).toBeInTheDocument();
    await user.click(within(dialog()).getByRole("button", { name: "Remove value 1" }));
    expect(values()).toEqual(["2", "3"]);
    expect(within(dialog()).queryByRole("button", { name: "Remove value 1" })).not.toBeInTheDocument();
    for (const [index, value] of ["4", "5", "6", "7"].entries()) {
      await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_ADD_LABEL }));
      const input = within(dialog()).getByRole("textbox", { name: `Value ${index + 3}` });
      await user.type(input, `${value}{Enter}`);
    }
    expect(values()).toHaveLength(6);
    expect(within(dialog()).queryByRole("button", { name: ESTIMATE_ADD_LABEL })).not.toBeInTheDocument();
  });

  it("edits inline by double-click and control, and Escape cancels", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByText("1", { selector: "[data-testid='estimate-value']" }));
    expect(within(dialog()).queryByRole("textbox")).not.toBeInTheDocument();
    await user.dblClick(within(dialog()).getByText("2", { selector: "[data-testid='estimate-value']" }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 2" });
    await user.type(input, "0");
    await user.keyboard("{Enter}");
    expect(values()).toEqual(["1", "20"]);
    await editValue(user, 0, "99", "escape");
    expect(values()).toEqual(["1", "20"]);
  });

  it("cancels an empty added row instead of leaving a blank value", async () => {
    const user = userEvent.setup();
    stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_ADD_LABEL }));
    await user.click(within(dialog()).getAllByRole("button", { name: "Cancel" })[0] as HTMLElement);
    expect(values()).toEqual(["1", "2"]);
    expect(within(dialog()).queryByRole("textbox")).not.toBeInTheDocument();
  });
});

describe("EstimateCreateDialog validation", () => {
  it("shows inline markers and blocks submit while a row is dirty", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await editValue(user, 0, "2");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    expect(within(dialog()).getByText("This value already exists in the set.")).toBeInTheDocument();
    expect(posts).toHaveLength(0);
  });

  it("refuses non-numeric and non-positive numeric values", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await editValue(user, 0, "abc");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    expect(within(dialog()).getByText("Enter a numeric value.")).toBeInTheDocument();
    expect(posts).toHaveLength(0);
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 1" }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 1" });
    await user.clear(input);
    await user.type(within(dialog()).getByRole("textbox", { name: "Value 1" }), "0");
    await user.click(within(dialog()).getByRole("button", { name: "Save" }));
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    expect(within(dialog()).getByText("Use a value greater than 0.")).toBeInTheDocument();
    expect(posts).toHaveLength(0);
  });

  it("refuses numeric category text and empty edits", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("radio", { name: "Categories" }));
    await pickStarter(user, "Custom");
    await editValue(user, 0, "123");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    expect(within(dialog()).getByText("Categories need text characters, not a number.")).toBeInTheDocument();
    expect(posts).toHaveLength(0);
    await user.click(within(dialog()).getByRole("button", { name: "Edit value 1" }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 1" });
    await user.clear(input);
    await user.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(within(dialog()).getByText("An estimate value cannot be empty.")).toBeInTheDocument();
    expect(within(dialog()).getByRole("textbox", { name: "Value 1" })).toBeInTheDocument();
    expect(within(dialog()).getByTestId("estimate-value")).toHaveTextContent("Hard");
    expect(posts).toHaveLength(0);
  });

  it("caps typed values at 20 characters and submits the capped text", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("radio", { name: "Categories" }));
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_ADD_LABEL }));
    const input = within(dialog()).getByRole("textbox", { name: "Value 3" });
    await user.type(input, "x".repeat(60));
    expect((input as HTMLInputElement).value).toHaveLength(20);
    await user.keyboard("{Enter}");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]?.estimate.name).toBe("Categories");
    expect(posts[0]?.estimate_points.at(-1)).toEqual({ key: 3, value: "x".repeat(20) });
  });

  it("blocks submit when the add form is open and empty", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_ADD_LABEL }));
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    expect(within(dialog()).getByText("An estimate value cannot be empty.")).toBeInTheDocument();
    expect(posts).toHaveLength(0);
  });
});

describe("EstimateCreateDialog submit", () => {
  it("submits trimmed values under the family name and closes", async () => {
    const user = userEvent.setup();
    const { posts } = stubApi();
    const { onCreated, onOpenChange } = renderDialog();
    await pickStarter(user, "Linear");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(notify.success).toHaveBeenCalled();
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

  it("submits the time family when the edition offers it", async () => {
    const user = userEvent.setup();
    editionHarness.flags = { [ESTIMATE_TIME_FAMILY_FLAG]: true };
    const { posts } = stubApi();
    renderDialog();
    await user.click(within(dialog()).getByRole("radio", { name: "Time" }));
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]?.estimate).toEqual({ name: "Time", type: "time", last_used: true });
    expect(posts[0]?.estimate_points).toEqual([
      { key: 1, value: "1" },
      { key: 2, value: "2" },
    ]);
  });

  it("keeps the dialog open with the draft when the server refuses", async () => {
    const user = userEvent.setup();
    stubApi({ status: 500 });
    const { onCreated, onOpenChange } = renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    await waitFor(() => expect(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL })).toBeEnabled());
    expect(notify.error).toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(dialog()).toHaveTextContent(ESTIMATE_DIALOG_TITLE);
    expect(values()).toEqual(["1", "2"]);
  });

  it("locks dismissal and editing while a submit is pending", async () => {
    const user = userEvent.setup();
    const api = stubApi({ hold: true });
    renderDialog();
    await pickStarter(user, "Custom");
    await user.click(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL }));
    expect(within(dialog()).getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(within(dialog()).getByRole("button", { name: /back/i })).toBeDisabled();
    expect(within(dialog()).getByRole("button", { name: "Edit value 1" })).toBeDisabled();
    expect(within(dialog()).queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    fireEvent.doubleClick(within(dialog()).getAllByTestId("estimate-value")[0] as HTMLElement);
    expect(within(dialog()).queryByRole("textbox", { name: "Value 1" })).not.toBeInTheDocument();
    api.release();
    await waitFor(() => expect(within(dialog()).getByRole("button", { name: ESTIMATE_SUBMIT_LABEL })).toBeEnabled());
  });
});
