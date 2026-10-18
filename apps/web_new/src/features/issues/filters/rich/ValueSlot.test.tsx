// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";

import {
  pickerProjectMembersQueryOptions,
  pickerStatesQueryOptions,
  pickerWorkspaceMembersQueryOptions,
} from "../../../../shared/pickers/index.js";
import { meQueryOptions } from "../../../../core/session/queries.js";
import { labelsQueryOptions } from "../../labels/api.js";
import { findField, richFilterFields, type RichCondition } from "./expression.js";
import { ValueSlot, selectedValues } from "./ValueSlot.js";

afterEach(() => {
  resetAppClient();
});

const FIELDS = richFilterFields();
const PRIORITY = findField(FIELDS, "priority")!;
const STATE = findField(FIELDS, "state_id")!;
const PROJECT = "123e4567-e89b-12d3-a456-426614174000";
const STATE_ID = "323e4567-e89b-12d3-a456-426614174002";

function condition(value: unknown = null): RichCondition {
  return { key: "a", property: "priority", operator: "in", negated: false, value };
}

function wrap(node: React.ReactElement, seedStates = false): React.ReactElement {
  createAppClient("https://api.test", (() => {
    throw new Error("unexpected network fetch in a seeded render");
  }) as never);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seedStates) {
    client.setQueryData(pickerStatesQueryOptions({} as never, "acme", PROJECT).queryKey, [
      {
        id: STATE_ID,
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
  }
  return (
    <QueryClientProvider client={client}>
      <React.Fragment>{node}</React.Fragment>
    </QueryClientProvider>
  );
}

describe("selectedValues", () => {
  it("reads arrays and ignores scalar or invalid values", () => {
    expect(selectedValues(["a", 1, "b"])).toEqual(["a", "b"]);
    expect(selectedValues("a")).toEqual(["a"]);
    expect(selectedValues("")).toEqual([]);
    expect(selectedValues(null)).toEqual([]);
  });
});

describe("ValueSlot", () => {
  it("shows the placeholder and picks a static multi value", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={PRIORITY}
          condition={condition()}
          onChange={onChange}
        />
      )
    );
    expect(screen.getByRole("button", { name: /Select value/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Select value/ }));
    const list = await screen.findByRole("listbox", { name: "Priority values" });
    expect(within(list).getAllByRole("option")).toHaveLength(5);
    await user.click(within(list).getByRole("option", { name: "Urgent" }));
    expect(onChange).toHaveBeenCalledWith(["urgent"]);
  });

  it("adds a second pick", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={PRIORITY}
          condition={condition(["urgent"])}
          onChange={onChange}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /Urgent/ }));
    await user.click(await screen.findByRole("option", { name: "High" }));
    expect(onChange).toHaveBeenLastCalledWith(["urgent", "high"]);
  });

  it("clears a re-picked value", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={PRIORITY}
          condition={condition(["urgent"])}
          onChange={onChange}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /Urgent/ }));
    await user.click(await screen.findByRole("option", { name: /Urgent/ }));
    expect(onChange).toHaveBeenLastCalledWith(null);
  });

  it("counts more than two picks", () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={PRIORITY}
          condition={condition(["urgent", "high", "low"])}
          onChange={() => {}}
        />
      )
    );
    expect(screen.getByRole("button", { name: /3 selected/ })).toBeInTheDocument();
  });

  it("lazy-loads workflow states and labels a resolved id", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={STATE}
          condition={{ ...condition([STATE_ID]), property: "state_id" }}
          onChange={onChange}
        />,
        true
      )
    );
    expect(screen.getByRole("button", { name: /Todo/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Todo/ }));
    expect(await screen.findByRole("option", { name: "Todo" })).toHaveAttribute("aria-selected", "true");
  });
});

describe("ValueSlot member and label sources", () => {
  const MEMBER_PROJECT = "123e4567-e89b-12d3-a456-426614174000";
  const ANA = "423e4567-e89b-12d3-a456-426614174003";
  const ZOE = "523e4567-e89b-12d3-a456-426614174004";
  const ME = "623e4567-e89b-12d3-a456-426614174005";

  function directoryMember(id: string, name: string) {
    return {
      id: `${id}-membership`,
      member: {
        id,
        first_name: name,
        last_name: "",
        avatar: "",
        avatar_url: null,
        display_name: name,
        is_bot: false,
      },
      role: 15,
      is_active: true,
    };
  }

  function projectMember(id: string) {
    return {
      id: `${id}-project`,
      role: 15,
      member: id,
      project: MEMBER_PROJECT,
      original_role: 15,
      created_at: "2026-01-01T00:00:00.000Z",
    };
  }

  function wrapWithCaches(node: React.ReactElement) {
    createAppClient("https://api.test", (() => {
      throw new Error("unexpected network fetch in a seeded render");
    }) as never);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(pickerProjectMembersQueryOptions({} as never, "acme", MEMBER_PROJECT).queryKey, [
      projectMember(ZOE),
      projectMember(ANA),
      projectMember(ME),
    ]);
    client.setQueryData(pickerWorkspaceMembersQueryOptions({} as never, "acme").queryKey, [
      directoryMember(ZOE, "Zoe"),
      directoryMember(ANA, "Ana"),
      directoryMember(ME, "Parity User"),
    ]);
    client.setQueryData(meQueryOptions({} as never).queryKey, {
      id: ME,
      email: "parity@example.com",
      first_name: "Parity",
      last_name: "User",
      avatar: "",
      avatar_url: null,
      display_name: "Parity User",
      is_bot: false,
    } as never);
    client.setQueryData(labelsQueryOptions({} as never, "acme", MEMBER_PROJECT).queryKey, [
      {
        id: "label-z",
        name: "Zeta",
        color: "#ff0000",
        project_id: MEMBER_PROJECT,
        workspace_id: MEMBER_PROJECT,
        parent: null,
        sort_order: 1,
      },
      {
        id: "label-a",
        name: "Alpha",
        color: "#00ff00",
        project_id: MEMBER_PROJECT,
        workspace_id: MEMBER_PROJECT,
        parent: null,
        sort_order: 2,
      },
    ]);
    return (
      <QueryClientProvider client={client}>
        <React.Fragment>{node}</React.Fragment>
      </QueryClientProvider>
    );
  }

  it("lists project members with You first and adds the picked id", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrapWithCaches(
        <ValueSlot
          workspaceSlug="acme"
          projectId={MEMBER_PROJECT}
          field={findField(FIELDS, "assignee_id")!}
          condition={{ key: "a", property: "assignee_id", operator: "in", negated: false, value: null }}
          onChange={onChange}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /Select value/ }));
    const list = await screen.findByRole("listbox", { name: "Assignees values" });
    expect(
      within(list)
        .getAllByRole("option")
        .map((option) => option.textContent)
    ).toEqual([expect.stringMatching(/You$/), expect.stringMatching(/Ana$/), expect.stringMatching(/Zoe$/)]);
    await user.click(within(list).getByRole("option", { name: /Ana/ }));
    expect(onChange).toHaveBeenCalledWith([ANA]);
  });

  it("orders applied members above the signed-in member", async () => {
    const user = userEvent.setup();
    render(
      wrapWithCaches(
        <ValueSlot
          workspaceSlug="acme"
          projectId={MEMBER_PROJECT}
          field={findField(FIELDS, "created_by_id")!}
          condition={{ key: "a", property: "created_by_id", operator: "in", negated: false, value: [ZOE] }}
          onChange={() => {}}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /Zoe/ }));
    const list = await screen.findByRole("listbox", { name: "Created by values" });
    expect(
      within(list)
        .getAllByRole("option")
        .map((option) => option.textContent)
    ).toEqual([expect.stringMatching(/Zoe$/), expect.stringMatching(/You$/), expect.stringMatching(/Ana$/)]);
  });

  it("lists labels with applied first and alphabetical order", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrapWithCaches(
        <ValueSlot
          workspaceSlug="acme"
          projectId={MEMBER_PROJECT}
          field={findField(FIELDS, "label_id")!}
          condition={{ key: "a", property: "label_id", operator: "in", negated: false, value: ["label-z"] }}
          onChange={onChange}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /Zeta/ }));
    const list = await screen.findByRole("listbox", { name: "Label values" });
    expect(
      within(list)
        .getAllByRole("option")
        .map((option) => option.textContent)
    ).toEqual(["Zeta", "Alpha"]);
    await user.click(within(list).getByRole("option", { name: /Alpha/ }));
    expect(onChange).toHaveBeenCalledWith(["label-z", "label-a"]);
  });

  it("shows the loading row while the member directory resolves", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      return new Promise(() => {});
    }) as never);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <ValueSlot
          workspaceSlug="acme"
          projectId={MEMBER_PROJECT}
          field={findField(FIELDS, "mention_id")!}
          condition={{ key: "a", property: "mention_id", operator: "in", negated: false, value: null }}
          onChange={() => {}}
        />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: /Select value/ }));
    expect((await screen.findAllByText("Loading values")).length).toBeGreaterThan(0);
  });
});

describe("ValueSlot failure and suspended rows", () => {
  it("reports a failed member read instead of an empty list", async () => {
    const user = userEvent.setup();
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      throw new Error("offline");
    }) as never);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "assignee_id")!}
          condition={{ key: "a", property: "assignee_id", operator: "in", negated: false, value: null }}
          onChange={() => {}}
        />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: /Select value/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load assignees values.");
  });
});
