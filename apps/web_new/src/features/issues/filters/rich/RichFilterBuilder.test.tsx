// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { IssuesLoaderProvider } from "../../loaders/loaderContext.js";
import type { IssuesEntity, PrefsRecord } from "../../prefs/entities.js";
import { prefsQueryOptions } from "../../prefs/prefsQuery.js";
import { useRichFilterBuilder, type RichFilterBuilderProps } from "./RichFilterBuilder.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ENTITY: IssuesEntity = { scope: "project", projectId: PROJECT };
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function record(richFilters: Record<string, unknown>): PrefsRecord & Record<string, unknown> {
  return {
    id: UUID,
    workspace: UUID,
    project: PROJECT,
    user: UUID,
    filters: {},
    display_filters: { layout: "list" },
    display_properties: {},
    rich_filters: richFilters,
    preferences: {},
    sort_order: 1,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

function setup(richFilters: Record<string, unknown>, options: { hold?: boolean; fail?: boolean } = {}) {
  const patches: Array<Record<string, unknown>> = [];
  let release = (): void => {};
  const gate = new Promise<void>((resolve) => (release = resolve));
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    if (init.method === "PATCH") {
      const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      patches.push(body);
      if (options.hold === true) await gate;
      if (options.fail === true) throw new Error("offline");
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(record(body["rich_filters"] as Record<string, unknown>)),
      };
    }
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(record(richFilters)) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(prefsQueryOptions({} as never, "acme", ENTITY).queryKey, record(richFilters));

  function Harness(): React.ReactElement {
    const { control, row } = useRichFilterBuilder({
      workspaceSlug: "acme",
      projectId: PROJECT,
      entity: ENTITY,
      hideCycle: false,
      hideModule: false,
    } satisfies RichFilterBuilderProps);
    return (
      <div>
        <div data-testid="control-slot">{control}</div>
        {row}
      </div>
    );
  }

  render(
    <QueryClientProvider client={queryClient}>
      <IssuesLoaderProvider>
        <Harness />
      </IssuesLoaderProvider>
    </QueryClientProvider>
  );
  return { patches, release, hold: options.hold === true };
}

afterEach(() => {
  resetAppClient();
});

describe("rich filter builder empty state", () => {
  it("reads Add filter and opens the property picker with every field", async () => {
    const user = userEvent.setup();
    setup({});
    expect(screen.queryByTestId("rich-filter-row")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("rich-filter-add"));
    const list = await screen.findByRole("listbox", { name: "Filter properties" });
    expect(
      within(list)
        .getAllByRole("option")
        .map((row) => row.textContent)
    ).toEqual(["State", "State group", "Priority"]);
  });

  it("adds a draft condition without patching an unchanged expression", async () => {
    const user = userEvent.setup();
    const { patches } = setup({});
    await user.click(screen.getByTestId("rich-filter-add"));
    await user.click(await screen.findByRole("option", { name: "Priority" }));

    expect(await screen.findByTestId("rich-filter-row")).toBeInTheDocument();
    expect(screen.getByTestId("rich-condition")).toHaveTextContent("Priority");
    expect(patches).toHaveLength(0);
  });
});

describe("rich filter builder populated state", () => {
  it("renders one pill per stored condition and a toggle", () => {
    setup({ and: [{ priority__in: "none" }, { state_group__in: "started" }] });
    expect(screen.getAllByTestId("rich-condition")).toHaveLength(2);
    expect(screen.getByTestId("rich-filter-toggle")).toHaveAttribute("aria-pressed", "true");
  });

  it("hides and shows the row from the header toggle", async () => {
    const user = userEvent.setup();
    setup({ priority__in: "none" });
    await user.click(screen.getByTestId("rich-filter-toggle"));
    expect(screen.queryByTestId("rich-filter-row")).not.toBeInTheDocument();
    expect(screen.getByTestId("rich-filter-toggle")).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByTestId("rich-filter-toggle"));
    expect(screen.getByTestId("rich-filter-row")).toBeInTheDocument();
  });

  it("removes only the chosen condition and patches the rest", async () => {
    const user = userEvent.setup();
    const { patches } = setup({ and: [{ priority__in: "none" }, { state_group__in: "started" }] });
    const first = screen.getAllByTestId("rich-condition")[0]!;
    await user.click(within(first).getByRole("button", { name: "Remove filter" }));
    expect(screen.getAllByTestId("rich-condition")).toHaveLength(1);
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]).toEqual({ rich_filters: { state_group__in: "started" } });
  });

  it("adds a second condition joined with AND and keeps the draft until it has a value", async () => {
    const user = userEvent.setup();
    const { patches } = setup({ priority__in: "none" });
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    await user.click(await screen.findByRole("option", { name: "State group" }));
    expect(screen.getAllByTestId("rich-condition")).toHaveLength(2);
    expect(screen.getByTestId("rich-filter-row")).toHaveTextContent("State group");
    // Nothing to persist yet: the new condition has no value.
    expect(patches).toHaveLength(0);

    const second = screen.getAllByTestId("rich-condition")[1]!;
    await user.click(within(second).getByRole("button", { name: /Select value/ }));
    await user.click(await screen.findByRole("option", { name: "Started" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]).toEqual({
      rich_filters: { and: [{ priority__in: "none" }, { state_group__in: ["started"] }] },
    });
  });

  it("re-picks the property and resets operator and value", async () => {
    const user = userEvent.setup();
    const { patches } = setup({ priority__in: "none" });
    const pill = screen.getByTestId("rich-condition");
    await user.click(within(pill).getByRole("button", { name: /Priority/ }));
    await user.click(await screen.findByRole("option", { name: "State group" }));
    expect(within(pill).getByRole("button", { name: /State group/ })).toBeInTheDocument();
    expect(within(pill).getByRole("button", { name: /Select value/ })).toBeInTheDocument();
    // The reset value makes the stored condition incomplete, so the old
    // filter leaves the record and the new one waits for a value.
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]).toEqual({ rich_filters: {} });
    expect(screen.getByTestId("rich-condition")).toHaveTextContent("State group");
  });

  it("stores a negated operator as a not wrapper", async () => {
    const user = userEvent.setup();
    const { patches } = setup({ priority__in: "none" });
    const pill = screen.getByTestId("rich-condition");
    await user.click(within(pill).getByRole("button", { name: /^is$/ }));
    await user.click(await screen.findByRole("option", { name: "is not" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]).toEqual({ rich_filters: { not: { priority__in: "none" } } });
  });

  it("renders a stored unknown property as an error pill with remove only", async () => {
    setup({ deleted_field__in: "x" });
    const invalid = screen.getByTestId("rich-condition-invalid");
    expect(invalid).toHaveTextContent("Unavailable filter");
    expect(within(invalid).getAllByRole("button")).toHaveLength(1);
  });

  it("marks used fields out of the add picker and shows exhaustion", async () => {
    const user = userEvent.setup();
    setup({ and: [{ state_id__in: [UUID] }, { state_group__in: "started" }, { priority__in: "none" }] });
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    const exhausted = await screen.findByRole("option", { name: "All filters applied" });
    expect(exhausted).toBeDisabled();
  });

  it("adopts a server echo without dropping the remaining draft condition", async () => {
    const user = userEvent.setup();
    const { patches, release, hold } = setup({ priority__in: "none" }, { hold: true });
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    await user.click(await screen.findByRole("option", { name: "State group" }));
    const second = screen.getAllByTestId("rich-condition")[1]!;
    await user.click(within(second).getByRole("button", { name: /Select value/ }));
    await user.click(await screen.findByRole("option", { name: "Started" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(hold).toBe(true);

    // A new half-built condition while the first PATCH is still in flight.
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    await user.click(await screen.findByRole("option", { name: "State" }));
    expect(screen.getAllByTestId("rich-condition")).toHaveLength(3);
    release();
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(screen.getAllByTestId("rich-condition")).toHaveLength(3);
  });

  it("rolls the row back when the preference patch fails", async () => {
    const user = userEvent.setup();
    const { patches } = setup({ and: [{ priority__in: "none" }, { state_group__in: "started" }] }, { fail: true });
    const first = screen.getAllByTestId("rich-condition")[0]!;
    await user.click(within(first).getByRole("button", { name: "Remove filter" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    await waitFor(() => expect(screen.getAllByTestId("rich-condition")).toHaveLength(2));
  });
});
