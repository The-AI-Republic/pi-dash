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
import { pickerStatesQueryOptions } from "../../../../shared/pickers/index.js";
import { IssuesLoaderProvider } from "../../loaders/loaderContext.js";
import type { IssuesEntity, PrefsRecord } from "../../prefs/entities.js";
import { prefsQueryOptions } from "../../prefs/prefsQuery.js";
import { richFilterFields } from "./expression.js";
import { sessionKeys } from "../../../../core/session/queries.js";
import { authKeys } from "../../../auth/queries.js";
import { useRichFilterBuilder, type RichFilterBuilderProps } from "./RichFilterBuilder.js";
import { FieldConfigRegistry } from "./temporary.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ENTITY: IssuesEntity = { scope: "project", projectId: PROJECT };
const UUID = "123e4567-e89b-12d3-a456-426614174000";
const VIEW_RECORD = {
  id: "323e4567-e89b-12d3-a456-426614174002",
  workspace: UUID,
  project: PROJECT,
  name: "Triage",
  description: "",
  query: {},
  filters: {},
  display_filters: { layout: "list" },
  display_properties: {},
  rich_filters: {},
  access: 0,
  sort_order: 1,
  logo_props: {},
  owned_by: UUID,
  is_locked: false,
  archived_at: null,
  created_by: UUID,
  updated_by: UUID,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

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
  return { patches, release, hold: options.hold === true, queryClient };
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
    ).toEqual([
      "State",
      "State Group",
      "Priority",
      "Assignees",
      "Created by",
      "Mentions",
      "Label",
      "Project",
      "Cycle",
      "Module",
      "Target date",
      "Start date",
    ]);
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

  it("opens the list of a newly added pick field without another click", async () => {
    const user = userEvent.setup();
    const { queryClient } = setup({});
    queryClient.setQueryData(pickerStatesQueryOptions({} as never, "acme", PROJECT).queryKey, [
      {
        id: "323e4567-e89b-12d3-a456-426614174002",
        name: "Todo",
        group: "unstarted",
        color: "#fff",
        project_id: PROJECT,
        workspace_id: PROJECT,
        default: false,
        description: "",
        sequence: 1,
      },
    ]);
    await user.click(screen.getByTestId("rich-filter-add"));
    await user.click(await screen.findByRole("option", { name: "State" }));

    // The empty slot is the first place to type or pick, so it lists by itself.
    const list = await screen.findByRole("listbox", { name: "State values" });
    expect(within(list).getByRole("option", { name: "Todo" })).toBeInTheDocument();
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
    await user.click(await screen.findByRole("option", { name: "State Group" }));
    expect(screen.getAllByTestId("rich-condition")).toHaveLength(2);
    expect(screen.getByTestId("rich-filter-row")).toHaveTextContent("State Group");
    // Nothing to persist yet: the new condition has no value.
    expect(patches).toHaveLength(0);

    const second = screen.getAllByTestId("rich-condition")[1]!;
    await user.click(within(second).getByRole("button", { name: /^--$/ }));
    await user.click(await screen.findByRole("option", { name: "Started" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]).toEqual({
      rich_filters: { and: [{ priority__in: "none" }, { state_group__exact: "started" }] },
    });
  });

  it("re-picks the property and resets operator and value", async () => {
    const user = userEvent.setup();
    const { patches } = setup({ priority__in: "none" });
    const pill = screen.getByTestId("rich-condition");
    await user.click(within(pill).getByRole("button", { name: /Priority/ }));
    await user.click(await screen.findByRole("option", { name: "State Group" }));
    expect(within(pill).getByRole("button", { name: /State Group/ })).toBeInTheDocument();
    expect(within(pill).getByRole("button", { name: /^--$/ })).toBeInTheDocument();
    // The reset value makes the stored condition incomplete, so the old
    // filter leaves the record and the new one waits for a value.
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]).toEqual({ rich_filters: {} });
    expect(screen.getByTestId("rich-condition")).toHaveTextContent("State Group");
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
    setup({
      and: richFilterFields().map((field) => ({ [`${field.stem}__in`]: "x" })),
    });
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    const exhausted = await screen.findByRole("option", { name: "All filters applied" });
    expect(exhausted).toBeDisabled();
  });

  it("adopts a server echo without dropping the remaining draft condition", async () => {
    const user = userEvent.setup();
    const { patches, release, hold } = setup({ priority__in: "none" }, { hold: true });
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    await user.click(await screen.findByRole("option", { name: "State Group" }));
    const second = screen.getAllByTestId("rich-condition")[1]!;
    await user.click(within(second).getByRole("button", { name: /^--$/ }));
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

  it("clear all drops every condition and stores the empty expression", async () => {
    const user = userEvent.setup();
    const { patches } = setup({ and: [{ priority__in: "none" }, { state_group__in: "started" }] });
    await user.click(screen.getByTestId("rich-clear-all"));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]).toEqual({ rich_filters: {} });
    // An empty expression hides the row and returns the Add control.
    await waitFor(() => expect(screen.getByTestId("rich-filter-add")).toBeInTheDocument());
  });
});

describe("view row wiring", () => {
  function ViewHarness(props: {
    richFilters: Record<string, unknown>;
    view?: RichFilterBuilderProps["view"];
    onViewUpdated?: (view: import("@pidash/api-client").SavedView) => void;
  }): React.ReactElement {
    const { row } = useRichFilterBuilder({
      workspaceSlug: "acme",
      projectId: PROJECT,
      entity: ENTITY,
      hideCycle: false,
      hideModule: false,
      view: props.view,
      onViewUpdated: props.onViewUpdated,
    });
    return <div>{row}</div>;
  }

  function setupView(
    richFilters: Record<string, unknown>,
    view?: RichFilterBuilderProps["view"],
    roles: { workspaceRole: number; projectRole: number | null } = { workspaceRole: 20, projectRole: 20 }
  ): { patches: Array<Record<string, unknown>>; onViewUpdated: ReturnType<typeof vi.fn> } {
    const patches: Array<Record<string, unknown>> = [];
    createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      if (init.method === "PATCH") {
        const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
        patches.push(body);
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ ...VIEW_RECORD, rich_filters: body["rich_filters"] }),
        };
      }
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(record(richFilters)) };
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(prefsQueryOptions({} as never, "acme", ENTITY).queryKey, record(richFilters));
    queryClient.setQueryData(sessionKeys.me, { id: VIEW_RECORD.owned_by });
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: UUID, name: "Acme", slug: "acme", logo_url: null, total_members: 3, role: roles.workspaceRole },
    ]);
    queryClient.setQueryData(
      authKeys.projectRoles("acme"),
      roles.projectRole === null ? {} : { [PROJECT]: roles.projectRole }
    );
    const onViewUpdated = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <IssuesLoaderProvider>
          <ViewHarness richFilters={richFilters} view={view} onViewUpdated={onViewUpdated} />
        </IssuesLoaderProvider>
      </QueryClientProvider>
    );
    return { patches, onViewUpdated };
  }

  it("offers Update view for a changed owned view and reports the patch payload", async () => {
    const user = userEvent.setup();
    const { patches, onViewUpdated } = setupView(
      { priority__in: "none" },
      {
        view: { ...VIEW_RECORD, rich_filters: { priority__in: "urgent" } } as never,
        isDefault: false,
      }
    );
    // The view's stored expression changes only when the row does: clear
    // the condition, then the owned view reads as changed.
    const pill = await screen.findByTestId("rich-condition");
    await user.click(within(pill).getByRole("button", { name: /^is$/ }));
    await user.click(await screen.findByRole("option", { name: "is not" }));
    await user.click(await screen.findByRole("button", { name: "Update view" }));
    await waitFor(() => expect(onViewUpdated).toHaveBeenCalledTimes(1));
    expect(patches).toHaveLength(1);
    expect(patches[0]?.["rich_filters"]).toEqual({ not: { priority__in: "urgent" } });
    expect(onViewUpdated.mock.calls[0]?.[0]).toMatchObject({ id: VIEW_RECORD.id });
  });

  it("hides the view actions from a workspace member who has not joined the project", async () => {
    setupView(
      { priority__in: "none" },
      {
        view: { ...VIEW_RECORD, rich_filters: { priority__in: "urgent" } } as never,
        isDefault: false,
      },
      { workspaceRole: 15, projectRole: null }
    );
    expect(await screen.findByTestId("rich-condition")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save as" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update view" })).not.toBeInTheDocument();
  });

  it("offers the view actions to a member of the project", async () => {
    const user = userEvent.setup();
    setupView(
      { priority__in: "none" },
      {
        view: { ...VIEW_RECORD, rich_filters: { priority__in: "urgent" } } as never,
        isDefault: false,
      },
      { workspaceRole: 15, projectRole: 15 }
    );
    expect(await screen.findByRole("button", { name: "Save as" })).toBeInTheDocument();
    // An untouched view reads clean; an edit turns the update on.
    expect(screen.queryByRole("button", { name: "Update view" })).not.toBeInTheDocument();
    const pill = screen.getByTestId("rich-condition");
    await user.click(within(pill).getByRole("button", { name: /^is$/ }));
    await user.click(await screen.findByRole("option", { name: "is not" }));
    expect(await screen.findByRole("button", { name: "Update view" })).toBeInTheDocument();
  });

  it("treats a view with no project as a workspace view", async () => {
    const user = userEvent.setup();
    const { patches } = setupView(
      { priority__in: "none" },
      {
        view: { ...VIEW_RECORD, project: null, rich_filters: { priority__in: "urgent" } } as never,
        isDefault: false,
      }
    );
    const pill = await screen.findByTestId("rich-condition");
    await user.click(within(pill).getByRole("button", { name: /^is$/ }));
    await user.click(await screen.findByRole("option", { name: "is not" }));
    await user.click(await screen.findByRole("button", { name: "Update view" }));
    await waitFor(() => expect(patches).toHaveLength(1));
  });

  it("offers Save as for a saved view and Save view on a bare list", async () => {
    const user = userEvent.setup();
    setupView(
      { priority__in: "none" },
      {
        view: {
          ...VIEW_RECORD,
          rich_filters: { priority__in: "none" },
          // A view saved from this row carries the full display snapshot.
          display_filters: { layout: "list", group_by: null, sub_group_by: null, order_by: "sort_order" },
        } as never,
        isDefault: false,
      }
    );
    expect(await screen.findByRole("button", { name: "Save as" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update view" })).not.toBeInTheDocument();
    const first = screen.getByTestId("rich-condition");
    await user.click(within(first).getByRole("button", { name: "Remove filter" }));
  });
});

describe("temporary builder instances", () => {
  function TemporaryHarness({
    temporary,
    registry,
  }: {
    temporary: boolean;
    registry?: FieldConfigRegistry | undefined;
  }): React.ReactElement {
    const { control, row, instanceId } = useRichFilterBuilder({
      workspaceSlug: "acme",
      projectId: PROJECT,
      entity: ENTITY,
      hideCycle: false,
      hideModule: false,
      temporary,
      temporaryRegistry: registry,
    });
    return (
      <div>
        <span data-testid="instance-id">{instanceId}</span>
        <div data-testid="control-slot">{control}</div>
        {row}
      </div>
    );
  }

  function mountTemporary(registry?: FieldConfigRegistry): void {
    // No transport at all: a persisted read would throw here, so mounting
    // proves the temporary branch never touches the prefs query.
    createAppClient("https://api.test", (() => {
      throw new Error("temporary instances must not read the server");
    }) as never);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <IssuesLoaderProvider>
          <TemporaryHarness temporary registry={registry} />
        </IssuesLoaderProvider>
      </QueryClientProvider>
    );
  }

  it("mints a TEMP- id and starts with an add control", () => {
    mountTemporary();
    expect(screen.getByTestId("instance-id")).toHaveTextContent(/^TEMP-\d+$/);
    expect(screen.getByTestId("rich-filter-add")).toBeInTheDocument();
    expect(screen.queryByTestId("rich-filter-row")).not.toBeInTheDocument();
  });

  it("keeps edits in memory and never patches", async () => {
    const user = userEvent.setup();
    mountTemporary();
    await user.click(screen.getByTestId("rich-filter-add"));
    await user.click(await screen.findByRole("option", { name: "Priority" }));
    const pill = await screen.findByTestId("rich-condition");
    await user.click(within(pill).getByRole("button", { name: /^--$/ }));
    await user.click(await screen.findByRole("option", { name: "Urgent" }));
    expect(screen.getByTestId("rich-condition")).toHaveTextContent("Urgent");
    // A second condition proves the in-memory list keeps accumulating.
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    await user.click(await screen.findByRole("option", { name: "State Group" }));
    expect(screen.getAllByTestId("rich-condition")).toHaveLength(2);
  });

  it("names persisted instances after their entity", () => {
    createAppClient("https://api.test", (() => {
      throw new Error("seeded instance ids need no read");
    }) as never);
    const scopes: IssuesEntity[] = [
      { scope: "project", projectId: PROJECT },
      { scope: "cycle", projectId: PROJECT, cycleId: "cycle-1" },
      { scope: "module", projectId: PROJECT, moduleId: "module-1" },
      { scope: "epics", projectId: PROJECT },
      { scope: "workspace" },
    ];
    const expected = [
      `project:${PROJECT}`,
      `cycle:${PROJECT}/cycle-1`,
      `module:${PROJECT}/module-1`,
      `epics:epics/${PROJECT}`,
      "workspace:workspace",
    ];
    scopes.forEach((entity, index) => {
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      queryClient.setQueryData(prefsQueryOptions({} as never, "acme", entity).queryKey, record({}));
      function ScopedHarness(): React.ReactElement {
        const { instanceId } = useRichFilterBuilder({
          workspaceSlug: "acme",
          projectId: PROJECT,
          entity,
          hideCycle: false,
          hideModule: false,
        });
        return <span data-testid="instance-id">{instanceId}</span>;
      }
      const { unmount } = render(
        <QueryClientProvider client={queryClient}>
          <IssuesLoaderProvider>
            <ScopedHarness />
          </IssuesLoaderProvider>
        </QueryClientProvider>
      );
      expect(screen.getByTestId("instance-id")).toHaveTextContent(expected[index]!);
      unmount();
    });
  });

  it("registers held fields in the temporary registry", async () => {
    const user = userEvent.setup();
    const registry = new FieldConfigRegistry();
    mountTemporary(registry);
    expect(registry.activeProperties()).toEqual([]);
    await user.click(screen.getByTestId("rich-filter-add"));
    await user.click(await screen.findByRole("option", { name: "Priority" }));
    await waitFor(() => expect(registry.activeProperties()).toEqual(["priority"]));
    // Clear all stays available in memory; save and update do not, since a
    // temp instance has no record to write.
    expect(screen.getByTestId("rich-clear-all")).toBeInTheDocument();
    expect(screen.queryByTestId("rich-save-view")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update view" })).not.toBeInTheDocument();
  });
});
