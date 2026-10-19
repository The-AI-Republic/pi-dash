// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
import { findField, parseRichExpression, richFilterFields, type RichCondition } from "./expression.js";
import { ValueSlot, selectedValues } from "./ValueSlot.js";

afterEach(() => {
  resetAppClient();
});

const FIELDS = richFilterFields();
const PRIORITY = findField(FIELDS, "priority")!;
const STATE = findField(FIELDS, "state_id")!;
const TARGET = findField(FIELDS, "target_date")!;
const START = findField(FIELDS, "start_date")!;
const PROJECT = "123e4567-e89b-12d3-a456-426614174000";
const STATE_ID = "323e4567-e89b-12d3-a456-426614174002";
const ME_ID = "11111111-1111-4111-8111-111111111111";
const ME_ROW = {
  id: ME_ID,
  email: "parity@example.com",
  username: "parity",
  first_name: "Parity",
  last_name: "User",
  display_name: "Parity User",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00.000Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};
const STATE_ROW = {
  id: STATE_ID,
  name: "Todo",
  group: "unstarted" as const,
  color: "#fff",
  project_id: PROJECT,
  workspace_id: PROJECT,
  default: false,
  description: "",
  sequence: 1,
};

function condition(value: unknown = null): RichCondition {
  return { key: "a", property: "priority", operator: "in", negated: false, value };
}

function wrap(node: React.ReactElement, seedStates = false): React.ReactElement {
  createAppClient("https://api.test", (() => {
    throw new Error("unexpected network fetch in a seeded render");
  }) as never);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seedStates) {
    client.setQueryData(pickerStatesQueryOptions({} as never, "acme", PROJECT).queryKey, [STATE_ROW]);
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

/** A client backed by routes, with no reference caches seeded. */
function coldWrap(node: React.ReactElement, routes: (url: string) => unknown) {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    const body = routes(url);
    if (body === null) throw new Error(`unexpected url ${url}`);
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    };
  }) as never);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <React.Fragment>{node}</React.Fragment>
    </QueryClientProvider>
  );
}

describe("ValueSlot stored values without an open popup", () => {
  it("resolves a persisted state name from a cold cache", async () => {
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={STATE}
          condition={{ ...condition([STATE_ID]), property: "state_id" }}
          onChange={() => {}}
        />,
        (url) => (url.endsWith("/states/") ? [STATE_ROW] : null)
      )
    );
    expect(await screen.findByRole("button", { name: /Todo/ })).toBeInTheDocument();
  });

  it("resolves a persisted member name from a cold cache", async () => {
    const ANA = "423e4567-e89b-12d3-a456-426614174003";
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "assignee_id")!}
          condition={{ key: "a", property: "assignee_id", operator: "in", negated: false, value: [ANA] }}
          onChange={() => {}}
        />,
        (url) => {
          if (url.endsWith("/api/users/me/")) return ME_ROW;
          if (url.includes("/projects/") && url.endsWith("/members/")) {
            return [
              {
                id: "723e4567-e89b-12d3-a456-426614174006",
                role: 15,
                member: ANA,
                project: PROJECT,
                original_role: 15,
                created_at: "2026-01-01T00:00:00.000Z",
              },
            ];
          }
          if (url.endsWith("/members/")) {
            return [
              {
                id: "823e4567-e89b-12d3-a456-426614174007",
                member: {
                  id: ANA,
                  first_name: "Ana",
                  last_name: "",
                  avatar: "",
                  avatar_url: null,
                  display_name: "Ana",
                  is_bot: false,
                },
                role: 15,
                is_active: true,
              },
            ];
          }
          return null;
        }
      )
    );
    expect(await screen.findByRole("button", { name: /Ana/ })).toBeInTheDocument();
  });

  it("resolves a persisted label name from a cold cache", async () => {
    const LABEL_ID = "b23e4567-e89b-12d3-a456-42661417400a";
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "label_id")!}
          condition={{ key: "a", property: "label_id", operator: "in", negated: false, value: [LABEL_ID] }}
          onChange={() => {}}
        />,
        (url) =>
          url.endsWith("/api/users/me/")
            ? ME_ROW
            : url.endsWith("/issue-labels/")
              ? [
                  {
                    id: LABEL_ID,
                    name: "Zeta",
                    color: "#ff0000",
                    project_id: PROJECT,
                    workspace_id: PROJECT,
                    parent: null,
                    sort_order: 1,
                  },
                ]
              : null
      )
    );
    expect(await screen.findByRole("button", { name: /Zeta/ })).toBeInTheDocument();
  });
});

describe("ValueSlot", () => {
  it("opens a pick slot that holds no value and commits the first pick from it", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={STATE}
          condition={{ key: "a", property: "state_id", operator: "in", negated: false, value: null }}
          onChange={onChange}
        />,
        true
      )
    );
    // No trigger click: an empty slot already shows its list.
    const list = await screen.findByRole("region", { name: "State" });
    await user.click(within(list).getByRole("checkbox", { name: "Todo" }));
    expect(onChange).toHaveBeenCalledWith([STATE_ID]);
  });

  it("reopens a stored one-pick exact as a complete, closed pick", () => {
    const [stored] = parseRichExpression({ priority__exact: "urgent" });
    render(
      wrap(
        <ValueSlot workspaceSlug="acme" projectId={PROJECT} field={PRIORITY} condition={stored!} onChange={() => {}} />
      )
    );
    expect(screen.getByRole("button", { name: /Urgent/ })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Priority" })).not.toBeInTheDocument();
  });

  it("adds a second pick to a stored one-pick exact without dropping the first", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const [stored] = parseRichExpression({ priority__exact: "urgent" });
    render(
      wrap(
        <ValueSlot workspaceSlug="acme" projectId={PROJECT} field={PRIORITY} condition={stored!} onChange={onChange} />
      )
    );
    await user.click(screen.getByRole("button", { name: /Urgent/ }));
    await user.click(await screen.findByRole("checkbox", { name: "High" }));
    expect(onChange).toHaveBeenLastCalledWith(["urgent", "high"]);
  });

  it("keeps a pick slot with a stored value closed until its trigger is used", () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={PRIORITY}
          condition={condition(["urgent"])}
          onChange={() => {}}
        />
      )
    );
    expect(screen.getByRole("button", { name: /Urgent/ })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Priority" })).not.toBeInTheDocument();
  });

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
    expect(screen.getByRole("button", { name: /^--$/ })).toBeInTheDocument();
    // An empty pick lists its options without another click; clicking the
    // trigger again would collapse it.
    const list = await screen.findByRole("region", { name: "Priority" });
    expect(within(list).getAllByRole("checkbox")).toHaveLength(5);
    await user.click(within(list).getByRole("checkbox", { name: "Urgent" }));
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
    await user.click(await screen.findByRole("checkbox", { name: "High" }));
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
    await user.click(await screen.findByRole("checkbox", { name: /Urgent/ }));
    expect(onChange).toHaveBeenLastCalledWith(null);
  });

  it("renders the fixed state-group set with icons", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "state_group")!}
          condition={{ key: "a", property: "state_group", operator: "in", negated: false, value: null }}
          onChange={() => {}}
        />
      )
    );
    const list = await screen.findByRole("region", { name: "State Group" });
    const options = within(list).getAllByRole("checkbox");
    expect(options).toHaveLength(5);
    for (const option of options) expect(option.querySelector("svg")).not.toBeNull();
  });

  it("shows an empty list for a field with no source and no static values", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={{
            stem: "custom",
            label: "Custom",
            kind: "multi",
            defaultOperator: "in",
            defaultNegated: false,
            operators: ["in"],
            allowNegative: true,
          }}
          condition={{ key: "a", property: "custom", operator: "in", negated: false, value: null }}
          onChange={() => {}}
        />
      )
    );
    expect(await screen.findByText("No matches found")).toBeInTheDocument();
  });

  it("leaves a static value with no icon unshaped", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={{
            stem: "custom",
            label: "Custom",
            kind: "multi",
            defaultOperator: "in",
            defaultNegated: false,
            operators: ["in"],
            allowNegative: true,
            staticValues: [{ value: "one", label: "One" }],
          }}
          condition={{ key: "a", property: "custom", operator: "in", negated: false, value: null }}
          onChange={() => {}}
        />
      )
    );
    const list = await screen.findByRole("region", { name: "Custom" });
    expect(within(list).getAllByRole("checkbox")).toHaveLength(1);
    expect(within(list).getByRole("checkbox", { name: "One" }).querySelector("svg")).toBeNull();
  });

  it("shows the first two picks plus the overflow count", () => {
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
    expect(screen.getByRole("button", { name: "Urgent, High +1 more" })).toBeInTheDocument();
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
    expect(await screen.findByRole("checkbox", { name: "Todo" })).toHaveAttribute("aria-checked", "true");
  });
});

const SINGLE_FIELD = {
  stem: "custom",
  label: "Custom",
  kind: "single" as const,
  defaultOperator: "exact",
  defaultNegated: false,
  operators: ["exact"],
  allowNegative: true,
  staticValues: [
    { value: "one", label: "One" },
    { value: "two", label: "Two" },
  ],
};

describe("ValueSlot single select", () => {
  it("replaces the value on a new pick instead of adding a second", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={SINGLE_FIELD}
          condition={{ key: "a", property: "custom", operator: "exact", negated: false, value: "one" }}
          onChange={onChange}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /One/ }));
    const group = await screen.findByRole("radiogroup", { name: "Custom values" });
    await user.click(within(group).getByRole("radio", { name: "Two" }));
    expect(onChange).toHaveBeenLastCalledWith("two");
    // One pick completes a single-choice slot, so its list closes.
    expect(screen.queryByRole("radiogroup", { name: "Custom values" })).not.toBeInTheDocument();
  });

  it("clears when the current value is picked again", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={SINGLE_FIELD}
          condition={{ key: "a", property: "custom", operator: "exact", negated: false, value: "one" }}
          onChange={onChange}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /One/ }));
    await user.click(await screen.findByRole("radio", { name: /One/ }));
    expect(onChange).toHaveBeenLastCalledWith(null);
  });

  it("shows the placeholder when no single value is set", () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={SINGLE_FIELD}
          condition={{ key: "a", property: "custom", operator: "exact", negated: false, value: null }}
          onChange={() => {}}
        />
      )
    );
    expect(screen.getByRole("button", { name: /^--/ })).toHaveAttribute("data-empty", "true");
  });

  it("shows one name rather than a list plus overflow", () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={SINGLE_FIELD}
          condition={{ key: "a", property: "custom", operator: "exact", negated: false, value: "one" }}
          onChange={() => {}}
        />
      )
    );
    expect(screen.getByRole("button", { name: /One/ })).toBeInTheDocument();
    expect(screen.queryByText(/more/)).not.toBeInTheDocument();
  });
});

describe("ValueSlot member and label sources", () => {
  const MEMBER_PROJECT = "123e4567-e89b-12d3-a456-426614174000";
  const ANA = "423e4567-e89b-12d3-a456-426614174003";
  const ZOE = "523e4567-e89b-12d3-a456-426614174004";
  const ME = "623e4567-e89b-12d3-a456-426614174005";

  function directoryMember(id: string, name: string, avatarUrl: string | null = null) {
    return {
      id: `${id}-membership`,
      member: {
        id,
        first_name: name,
        last_name: "",
        avatar: "",
        avatar_url: avatarUrl,
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
    const list = await screen.findByRole("region", { name: "Assignees" });
    expect(
      within(list)
        .getAllByRole("checkbox")
        .map((option) => option.textContent)
    ).toEqual([expect.stringMatching(/You$/), expect.stringMatching(/Ana$/), expect.stringMatching(/Zoe$/)]);
    await user.click(within(list).getByRole("checkbox", { name: /Ana/ }));
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
    const list = await screen.findByRole("region", { name: "Created by" });
    expect(
      within(list)
        .getAllByRole("checkbox")
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
    const list = await screen.findByRole("region", { name: "Label" });
    expect(
      within(list)
        .getAllByRole("checkbox")
        .map((option) => option.textContent)
    ).toEqual(["Zeta", "Alpha"]);
    await user.click(within(list).getByRole("checkbox", { name: /Alpha/ }));
    expect(onChange).toHaveBeenCalledWith(["label-z", "label-a"]);
  });

  it("renders a member avatar image when the directory has one", async () => {
    const AVATAR = "https://cdn.test/ana.png";
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "assignee_id")!}
          condition={{ key: "a", property: "assignee_id", operator: "in", negated: false, value: null }}
          onChange={() => {}}
        />,
        (url) => {
          if (url.endsWith("/api/users/me/")) return ME_ROW;
          if (url.includes("/projects/") && url.endsWith("/members/")) {
            return [
              {
                id: "923e4567-e89b-12d3-a456-426614174008",
                role: 15,
                member: ANA,
                project: PROJECT,
                original_role: 15,
                created_at: "2026-01-01T00:00:00.000Z",
              },
            ];
          }
          if (url.endsWith("/members/")) {
            return [
              {
                id: "a23e4567-e89b-12d3-a456-426614174009",
                member: {
                  id: ANA,
                  first_name: "Ana",
                  last_name: "Roa",
                  avatar: "",
                  avatar_url: AVATAR,
                  display_name: "Ana Roa",
                  is_bot: false,
                },
                role: 15,
                is_active: true,
              },
            ];
          }
          return null;
        }
      )
    );
    const option = await screen.findByRole("checkbox", { name: /Ana/ });
    expect(option.querySelector(`img[src="${AVATAR}"]`)).not.toBeNull();
  });

  it("shows the loading row while the member directory resolves", async () => {
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
    expect((await screen.findAllByText("Loading values")).length).toBeGreaterThan(0);
  });
});

describe("ValueSlot project, cycle and module sources", () => {
  const CYCLE = "c23e4567-e89b-12d3-a456-42661417400c";
  const MODULE = "d23e4567-e89b-12d3-a456-42661417400d";

  function projectRow(id: string, name: string, identifier: string) {
    return {
      id,
      name,
      identifier,
      sort_order: null,
      logo_props: { in_use: "emoji", emoji: { value: "🌱" } },
      member_role: 15,
      intake_count: 0,
      archived_at: null,
      workspace: PROJECT,
      cycle_view: true,
      issue_views_view: true,
      module_view: true,
      page_view: false,
      inbox_view: false,
      is_default: false,
      guest_view_all_features: false,
      project_lead: null,
      network: 0,
      created_at: "2026-01-01T00:00:00.000Z",
      updated_at: "2026-01-01T00:00:00.000Z",
      created_by: null,
      updated_by: null,
    };
  }

  const ROUTES: Record<string, unknown> = {
    "/projects/": [projectRow(PROJECT, "Apollo", "APO")],
    "/cycles/": [{ id: CYCLE, project_id: PROJECT, name: "Sprint 4", is_favorite: false, status: "ACTIVE" }],
    "/modules/": [{ id: MODULE, project_id: PROJECT, name: "Billing", status: "planned", is_favorite: false }],
  };

  function route(url: string): unknown | null {
    for (const [suffix, body] of Object.entries(ROUTES)) {
      if (url.endsWith(suffix)) return body;
    }
    return null;
  }

  it.each([
    ["project_id", PROJECT, "Apollo"],
    ["cycle_id", CYCLE, "Sprint 4"],
    ["module_id", MODULE, "Billing"],
  ] as const)("lists %s options only once opened", async (stem, id, name) => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, stem)!}
          condition={{ key: "a", property: stem, operator: "in", negated: false, value: null }}
          onChange={onChange}
        />,
        route
      )
    );
    // Lazy: the empty slot opens by itself and only then reads.
    expect(screen.getByRole("button", { name: /^--$/ })).toBeInTheDocument();
    expect(await screen.findByRole("checkbox", { name })).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox", { name }));
    expect(onChange).toHaveBeenCalledWith([id]);
  });

  it.each([
    ["project_id", PROJECT, "Apollo"],
    ["cycle_id", CYCLE, "Sprint 4"],
    ["module_id", MODULE, "Billing"],
  ] as const)("resolves a stored %s id to its name", async (stem, id, name) => {
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, stem)!}
          condition={{ key: "a", property: stem, operator: "in", negated: false, value: [id] }}
          onChange={() => {}}
        />,
        route
      )
    );
    expect(await screen.findByRole("button", { name: new RegExp(name) })).toBeInTheDocument();
  });
});

describe("ValueSlot failure and suspended rows", () => {
  it("reports a failed member read instead of an empty list", async () => {
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
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load assignees values.");
  });

  it("renders a suspended member disabled and ignores a pick", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const ZOE = "523e4567-e89b-12d3-a456-426614174004";
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "assignee_id")!}
          condition={{ key: "a", property: "assignee_id", operator: "in", negated: false, value: null }}
          onChange={onChange}
        />,
        (url) => {
          if (url.endsWith("/api/users/me/")) return ME_ROW;
          if (url.includes("/projects/") && url.endsWith("/members/")) {
            return [
              {
                id: "923e4567-e89b-12d3-a456-426614174008",
                role: 15,
                member: ZOE,
                project: PROJECT,
                original_role: 15,
                created_at: "2026-01-01T00:00:00.000Z",
              },
            ];
          }
          if (url.endsWith("/members/")) {
            return [
              {
                id: "a23e4567-e89b-12d3-a456-426614174009",
                member: {
                  id: ZOE,
                  first_name: "Zoe",
                  last_name: "",
                  avatar: "",
                  avatar_url: null,
                  display_name: "Zoe",
                  is_bot: false,
                },
                role: 15,
                is_active: false,
              },
            ];
          }
          return null;
        }
      )
    );
    const suspended = await screen.findByRole("checkbox", { name: /Zoe/ });
    expect(suspended).toBeDisabled();
    await user.click(suspended);
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("ValueSlot date editors", () => {
  it("opens the empty date slot by itself and closes after a day is picked", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={TARGET}
          condition={{ ...condition(null), property: "target_date", operator: "exact" }}
          onChange={onChange}
        />
      )
    );
    const calendar = await screen.findByTestId("rich-date-calendar");
    const firstDay = within(calendar).getAllByRole("button", { name: /^\w+ \d+, \d{4}$/ })[0]!;
    await user.click(firstDay);
    expect(onChange).toHaveBeenCalledWith(firstDay.getAttribute("data-day"));
    await waitFor(() => expect(screen.queryByTestId("rich-date-calendar")).not.toBeInTheDocument());
  });

  it("labels a set date and offers no clear control", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={TARGET}
          condition={{ ...condition("2026-02-10"), property: "target_date", operator: "exact" }}
          onChange={() => {}}
        />
      )
    );
    const trigger = screen.getByRole("button", { name: /2026-02-10/ });
    expect(trigger).toHaveAttribute("data-empty", "false");
    fireEvent.click(trigger);
    const calendar = await screen.findByTestId("rich-date-calendar");
    expect(within(calendar).queryByRole("button", { name: /clear/i })).not.toBeInTheDocument();
  });

  it("disables days outside the field's bounds", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={{ ...TARGET, min: "2026-02-05", max: "2026-02-20" }}
          condition={{ ...condition("2026-02-10"), property: "target_date", operator: "exact" }}
          onChange={() => {}}
        />
      )
    );
    fireEvent.click(screen.getByRole("button", { name: /2026-02-10/ }));
    const calendar = await screen.findByTestId("rich-date-calendar");
    expect(within(calendar).getByRole("button", { name: "February 5, 2026" })).toBeEnabled();
    expect(within(calendar).getByRole("button", { name: "February 4, 2026" })).toBeDisabled();
    expect(within(calendar).getByRole("button", { name: "February 20, 2026" })).toBeEnabled();
    expect(within(calendar).getByRole("button", { name: "February 21, 2026" })).toBeDisabled();
  });

  it("bounds the range calendar as well", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={{ ...START, min: "2026-02-05", max: "2026-02-20" }}
          condition={{ ...condition(["2026-02-10", "2026-02-12"]), property: "start_date", operator: "range" }}
          onChange={() => {}}
        />
      )
    );
    fireEvent.click(screen.getByRole("button", { name: /2026-02-10/ }));
    const calendar = await screen.findByTestId("rich-range-calendar");
    expect(within(calendar).getByRole("button", { name: "February 4, 2026" })).toBeDisabled();
    expect(within(calendar).getByRole("button", { name: "February 21, 2026" })).toBeDisabled();
  });

  it("opens an incomplete range, warns, and commits only once both ends land", async () => {
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{ ...condition(["2026-02-10", "2026-02-12"]), property: "start_date", operator: "range" }}
          onChange={onChange}
        />
      )
    );
    expect(screen.getByTestId("rich-value-trigger").parentElement).toHaveAttribute("data-incomplete", "false");
    fireEvent.click(screen.getByRole("button", { name: /2026-02-10/ }));
    const calendar = await screen.findByTestId("rich-range-calendar");
    expect(within(calendar).getByRole("button", { name: "February 10, 2026" })).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps the first end in the slot and commits nothing", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{ ...condition(null), property: "start_date", operator: "range" }}
          onChange={onChange}
        />
      )
    );
    const calendar = await screen.findByTestId("rich-range-calendar");
    const day = within(calendar).getAllByRole("button", { name: /^\w+ \d+, \d{4}$/ })[0]!;
    await user.click(day);
    // The host value is untouched until both ends exist.
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("rich-range-calendar")).toBeInTheDocument();
    expect(screen.getByTestId("rich-value-trigger").parentElement).toHaveAttribute("data-incomplete", "true");
    expect(screen.getByTestId("rich-value-trigger")).toHaveTextContent(`${day.getAttribute("data-day")} → …`);
  });

  it("commits both ends when the pending range completes", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{ ...condition(null), property: "start_date", operator: "range" }}
          onChange={onChange}
        />
      )
    );
    const calendar = await screen.findByTestId("rich-range-calendar");
    const days = within(calendar).getAllByRole("button", { name: /^\w+ \d+, \d{4}$/ });
    const first = days[5]!;
    const last = days[9]!;
    await user.click(first);
    await user.click(last);
    expect(onChange).toHaveBeenCalledWith([first.getAttribute("data-day"), last.getAttribute("data-day")]);
    await waitFor(() => expect(screen.queryByTestId("rich-range-calendar")).not.toBeInTheDocument());
  });

  it("labels a start-only range with the open end", () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{ ...condition(["2026-02-10", null]), property: "start_date", operator: "range" }}
          onChange={() => {}}
        />
      )
    );
    expect(screen.getByTestId("rich-value-trigger")).toHaveTextContent("2026-02-10 → …");
    expect(screen.getByTestId("rich-value-trigger").parentElement).toHaveAttribute("data-incomplete", "true");
  });

  it("labels a complete range with both ends", () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{ ...condition(["2026-02-10", "2026-02-12"]), property: "start_date", operator: "range" }}
          onChange={() => {}}
        />
      )
    );
    expect(screen.getByTestId("rich-value-trigger")).toHaveTextContent("2026-02-10 → 2026-02-12");
    expect(screen.getByRole("button", { name: /2026-02-10/ })).toHaveAttribute("data-empty", "false");
  });

  it("edits one day for a start date under its exact lookup", async () => {
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{ ...condition("2026-02-10"), property: "start_date", operator: "exact" }}
          onChange={onChange}
        />
      )
    );
    expect(screen.getByRole("button", { name: /2026-02-10/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /2026-02-10/ }));
    // The one-day editor opens a single calendar, not the range grid.
    const calendar = await screen.findByTestId("rich-date-calendar");
    expect(screen.queryByTestId("rich-range-calendar")).not.toBeInTheDocument();
    await userEvent.setup().click(within(calendar).getByRole("button", { name: "February 12, 2026" }));
    expect(onChange).toHaveBeenCalledWith("2026-02-12");
  });

  it("opens the single-day calendar by itself for an empty exact start date", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{ ...condition(null), property: "start_date", operator: "exact" }}
          onChange={() => {}}
        />
      )
    );
    expect(await screen.findByTestId("rich-date-calendar")).toBeInTheDocument();
  });

  it("renders a stored scalar start date under exact", () => {
    const [stored] = parseRichExpression({ start_date__exact: "2026-02-10" });
    render(
      wrap(<ValueSlot workspaceSlug="acme" projectId={PROJECT} field={START} condition={stored!} onChange={() => {}} />)
    );
    expect(screen.getByTestId("rich-value-trigger")).toHaveTextContent("2026-02-10");
    expect(screen.getByTestId("rich-value-trigger").parentElement).not.toHaveAttribute("data-incomplete", "true");
    expect(screen.queryByTestId("rich-date-calendar")).not.toBeInTheDocument();
  });

  it("bounds the single-day calendar for an exact start date", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={{ ...START, min: "2026-02-05", max: "2026-02-20" }}
          condition={{ ...condition("2026-02-10"), property: "start_date", operator: "exact" }}
          onChange={() => {}}
        />
      )
    );
    fireEvent.click(screen.getByRole("button", { name: /2026-02-10/ }));
    const calendar = await screen.findByTestId("rich-date-calendar");
    expect(within(calendar).getByRole("button", { name: "February 4, 2026" })).toBeDisabled();
    expect(within(calendar).getByRole("button", { name: "February 21, 2026" })).toBeDisabled();
  });

  it("renders both ends of a range read back from its stored string", () => {
    const [stored] = parseRichExpression({ start_date__range: "2026-02-10,2026-02-12" });
    render(
      wrap(<ValueSlot workspaceSlug="acme" projectId={PROJECT} field={START} condition={stored!} onChange={() => {}} />)
    );
    expect(screen.getByTestId("rich-value-trigger")).toHaveTextContent("2026-02-10 → 2026-02-12");
    expect(screen.getByTestId("rich-value-trigger").parentElement).not.toHaveAttribute("data-incomplete", "true");
    expect(screen.queryByTestId("rich-range-calendar")).not.toBeInTheDocument();
  });

  it("reopens a stored target-date range as both ends", () => {
    const [stored] = parseRichExpression({ target_date__range: "2026-02-10,2026-02-12" });
    expect(stored).toMatchObject({ property: "target_date", operator: "range", value: ["2026-02-10", "2026-02-12"] });
    render(
      wrap(
        <ValueSlot workspaceSlug="acme" projectId={PROJECT} field={TARGET} condition={stored!} onChange={() => {}} />
      )
    );
    expect(screen.getByTestId("rich-value-trigger")).toHaveTextContent("2026-02-10 → 2026-02-12");
    expect(screen.getByTestId("rich-value-trigger").parentElement).not.toHaveAttribute("data-incomplete", "true");
    expect(screen.queryByTestId("rich-range-calendar")).not.toBeInTheDocument();
  });
});

describe("ValueSlot cycle lifecycle and ordering", () => {
  const RUNNING = "c23e4567-e89b-12d3-a456-42661417400e";
  const UPCOMING = "c23e4567-e89b-12d3-a456-42661417400f";

  function cycleRow(id: string, name: string, status: string | undefined) {
    return { id, project_id: PROJECT, name, is_favorite: false, ...(status === undefined ? {} : { status }) };
  }

  function renderCycles(node: React.ReactElement) {
    return render(
      coldWrap(node, (url) => {
        if (url.endsWith("/cycles/")) {
          return [
            cycleRow(UPCOMING, "Zulu", "UPCOMING"),
            cycleRow(RUNNING, "alpha", "ACTIVE"),
            cycleRow("c23e4567-e89b-12d3-a456-42661417400a", "Beta", "DRAFT"),
            cycleRow("c23e4567-e89b-12d3-a456-42661417400b", "Gone", "COMPLETED"),
          ];
        }
        return null;
      })
    );
  }

  it("lists completed cycles with their glyph and pulses only the running one", async () => {
    renderCycles(
      <ValueSlot
        workspaceSlug="acme"
        projectId={PROJECT}
        field={findField(FIELDS, "cycle_id")!}
        condition={{ key: "a", property: "cycle_id", operator: "in", negated: false, value: [RUNNING] }}
        onChange={() => {}}
      />
    );
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: /c23e4567-e89b-12d3-a456-42661417400e/ }));
    const completed = screen.getByRole("checkbox", { name: "Gone" });
    expect(completed.querySelector('[data-testid="cycle-glyph-completed"]')).toHaveAttribute("data-running", "false");
    const running = screen.getByRole("checkbox", { name: "alpha" });
    expect(running.querySelector('[data-testid="cycle-glyph-current"]')).toHaveAttribute("data-running", "true");
    expect(screen.getByTestId("cycle-glyph-upcoming")).toHaveAttribute("data-running", "false");
    expect(screen.getByTestId("cycle-glyph-draft")).toHaveAttribute("data-running", "false");
  });

  it("orders the applied cycle first and the rest alphabetically", async () => {
    renderCycles(
      <ValueSlot
        workspaceSlug="acme"
        projectId={PROJECT}
        field={findField(FIELDS, "cycle_id")!}
        condition={{ key: "a", property: "cycle_id", operator: "in", negated: false, value: [RUNNING] }}
        onChange={() => {}}
      />
    );
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: /c23e4567-e89b-12d3-a456-42661417400e/ }));
    expect(screen.getAllByRole("checkbox").map((option) => option.textContent?.trim())).toEqual([
      "alpha",
      "Beta",
      "Gone",
      "Zulu",
    ]);
  });
});

describe("ValueSlot project and module ordering", () => {
  it("orders applied projects first, then names alphabetically", async () => {
    const user = userEvent.setup();
    const SECOND = "223e4567-e89b-12d3-a456-426614174002";
    function row(id: string, name: string, identifier: string) {
      return {
        id,
        name,
        identifier,
        sort_order: null,
        logo_props: {},
        member_role: 15,
        intake_count: 0,
        archived_at: null,
        workspace: PROJECT,
        cycle_view: true,
        issue_views_view: true,
        module_view: true,
        page_view: false,
        inbox_view: false,
        is_default: false,
        guest_view_all_features: false,
        project_lead: null,
        network: 0,
        created_at: "2026-01-01T00:00:00.000Z",
        updated_at: "2026-01-01T00:00:00.000Z",
        created_by: null,
        updated_by: null,
      };
    }
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "project_id")!}
          condition={{ key: "a", property: "project_id", operator: "in", negated: false, value: [SECOND] }}
          onChange={() => {}}
        />,
        (url) =>
          url.endsWith("/projects/")
            ? [
                row(PROJECT, "zephyr", "ZEP"),
                row(SECOND, "Orion", "ORI"),
                row("323e4567-e89b-12d3-a456-426614174003", "Apollo", "APO"),
              ]
            : null
      )
    );
    await user.click(await screen.findByRole("button", { name: /223e4567-e89b-12d3-a456-426614174002/ }));
    // Each row leads with its logo mark: the identifier's initial here.
    expect(screen.getAllByRole("checkbox").map((option) => option.textContent?.trim())).toEqual([
      "OOrion",
      "AApollo",
      "Zzephyr",
    ]);
  });

  it("orders applied modules first, then names alphabetically", async () => {
    const user = userEvent.setup();
    const SECOND = "d23e4567-e89b-12d3-a456-42661417400e";
    function row(id: string, name: string) {
      return { id, project_id: PROJECT, name, status: "planned", is_favorite: false };
    }
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "module_id")!}
          condition={{ key: "a", property: "module_id", operator: "in", negated: false, value: [SECOND] }}
          onChange={() => {}}
        />,
        (url) =>
          url.endsWith("/modules/")
            ? [
                row("d23e4567-e89b-12d3-a456-42661417400d", "zebra"),
                row(SECOND, "Billing"),
                row("d23e4567-e89b-12d3-a456-42661417400f", "Analytics"),
              ]
            : null
      )
    );
    await user.click(await screen.findByRole("button", { name: /d23e4567-e89b-12d3-a456-42661417400e/ }));
    expect(screen.getAllByRole("checkbox").map((option) => option.textContent?.trim())).toEqual([
      "Billing",
      "Analytics",
      "zebra",
    ]);
  });
});

describe("ValueSlot shared section chrome", () => {
  it("shows the field title with its applied count and reads rows as checkboxes", async () => {
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={PRIORITY}
          condition={condition(["urgent", "high"])}
          onChange={() => {}}
        />
      )
    );
    fireEvent.click(screen.getByRole("button", { name: /Urgent, High/ }));
    const section = await screen.findByRole("region", { name: "Priority" });
    expect(within(section).getByRole("heading", { name: "Priority (2)" })).toBeInTheDocument();
    expect(within(section).getByRole("checkbox", { name: "Urgent" })).toHaveAttribute("aria-checked", "true");
    expect(within(section).getByRole("checkbox", { name: "High" })).toHaveAttribute("aria-checked", "true");
    expect(within(section).getByRole("checkbox", { name: "Low" })).toHaveAttribute("aria-checked", "false");
  });

  it("searches the section by its own box and shows the no-matches placeholder", async () => {
    const user = userEvent.setup();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={PRIORITY}
          condition={condition()}
          onChange={() => {}}
        />
      )
    );
    const section = await screen.findByRole("region", { name: "Priority" });
    const search = within(section).getByRole("searchbox", { name: "Search Priority" });
    await user.type(search, "med");
    expect(within(section).getAllByRole("checkbox")).toHaveLength(1);
    expect(within(section).getByRole("checkbox", { name: "Medium" })).toBeInTheDocument();
    await user.clear(search);
    await user.type(search, "zzz");
    expect(within(section).queryAllByRole("checkbox")).toHaveLength(0);
    expect(within(section).getByText("No matches found")).toBeInTheDocument();
  });

  it("collapses and reopens the section through its chevron", async () => {
    const user = userEvent.setup();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={PRIORITY}
          condition={condition()}
          onChange={() => {}}
        />
      )
    );
    const section = await screen.findByRole("region", { name: "Priority" });
    await user.click(within(section).getByRole("button", { name: "Collapse Priority section" }));
    expect(within(section).queryAllByRole("checkbox")).toHaveLength(0);
    await user.click(within(section).getByRole("button", { name: "Expand Priority section" }));
    expect(within(section).getAllByRole("checkbox")).toHaveLength(5);
  });
});

describe("ValueSlot section cap", () => {
  it("reads back at five rows and reveals the rest with View all", async () => {
    const user = userEvent.setup();
    function row(index: number) {
      return {
        id: `d23e4567-e89b-12d3-a456-42661417400${index.toString(16)}`,
        project_id: PROJECT,
        name: `Module ${index}`,
        status: "planned",
        is_favorite: false,
      };
    }
    const modules = Array.from({ length: 7 }, (_, index) => row(index));
    render(
      coldWrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={findField(FIELDS, "module_id")!}
          condition={{ key: "a", property: "module_id", operator: "in", negated: false, value: [] }}
          onChange={() => {}}
        />,
        (url) => (url.endsWith("/modules/") ? modules : null)
      )
    );
    // An empty slot opens by itself, reading at the shared cap.
    await screen.findAllByRole("checkbox");
    expect(screen.getAllByRole("checkbox")).toHaveLength(5);
    await user.click(screen.getByRole("button", { name: "View all" }));
    expect(screen.getAllByRole("checkbox")).toHaveLength(7);
    expect(screen.getByRole("button", { name: "View less" })).toBeInTheDocument();
  });
});

describe("ValueSlot date presets and custom range", () => {
  it("ticking Custom opens the calendar and focuses its first day", async () => {
    const user = userEvent.setup();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={TARGET}
          condition={{ ...condition("2_weeks;after;fromnow"), property: "target_date", operator: "exact" }}
          onChange={() => {}}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /2 weeks from now/ }));
    await user.click(screen.getByRole("checkbox", { name: "Custom" }));
    await waitFor(() => {
      const focused = document.activeElement;
      expect(focused).not.toBeNull();
      expect(focused).toHaveAttribute("data-day");
      expect(screen.getByTestId("rich-date-calendar").contains(focused)).toBe(true);
    });
  });

  it("toggles a relative window and keeps the custom day", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={TARGET}
          condition={{ ...condition("2026-02-10"), property: "target_date", operator: "exact" }}
          onChange={onChange}
        />
      )
    );
    fireEvent.click(screen.getByRole("button", { name: /2026-02-10/ }));
    await screen.findByTestId("rich-date-calendar");
    await user.click(screen.getByRole("checkbox", { name: "2 weeks from now" }));
    expect(onChange).toHaveBeenLastCalledWith(["2026-02-10", "2_weeks;after;fromnow"]);
  });

  it("removes a relative window when it is ticked off", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={TARGET}
          condition={{ ...condition("1_weeks;after;fromnow"), property: "target_date", operator: "exact" }}
          onChange={onChange}
        />
      )
    );
    expect(screen.getByRole("button", { name: /1 week from now/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /1 week from now/ }));
    await user.click(screen.getByRole("checkbox", { name: "1 week from now" }));
    expect(onChange).toHaveBeenLastCalledWith(null);
  });

  it("unticks the Custom row to drop the custom range and keep the presets", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{
            ...condition(["1_weeks;after;fromnow", "2026-02-01;after", "2026-02-10;before"]),
            property: "start_date",
            operator: "range",
          }}
          onChange={onChange}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /2026-02-01 → 2026-02-10/ }));
    const custom = screen.getByRole("checkbox", { name: "Custom" });
    expect(custom).toHaveAttribute("aria-checked", "true");
    await user.click(custom);
    expect(onChange).toHaveBeenLastCalledWith("1_weeks;after;fromnow");
  });

  it("reads the Custom row as unticked when the value holds only presets", async () => {
    const user = userEvent.setup();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={TARGET}
          condition={{ ...condition("2_months;after;fromnow"), property: "target_date", operator: "exact" }}
          onChange={() => {}}
        />
      )
    );
    await user.click(screen.getByRole("button", { name: /2 months from now/ }));
    expect(screen.getByRole("checkbox", { name: "Custom" })).toHaveAttribute("aria-checked", "false");
  });

  it("keeps a relative window and adds a custom range under the between lookup", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      wrap(
        <ValueSlot
          workspaceSlug="acme"
          projectId={PROJECT}
          field={START}
          condition={{
            ...condition("1_months;after;fromnow"),
            property: "start_date",
            operator: "range",
          }}
          onChange={onChange}
        />
      )
    );
    // A preset alone completes the slot, so it stays closed until opened.
    const trigger = screen.getByRole("button", { name: /1 month from now/ });
    expect(trigger).toHaveAttribute("data-empty", "false");
    await user.click(trigger);
    const calendar = await screen.findByTestId("rich-range-calendar");
    const days = within(calendar).getAllByRole("button", { name: /^\w+ \d+, \d{4}$/ });
    await user.click(days[9]!);
    await user.click(days[11]!);
    const [first, second] = [days[9]!.getAttribute("data-day"), days[11]!.getAttribute("data-day")];
    expect(onChange).toHaveBeenLastCalledWith(["1_months;after;fromnow", first, second]);
  });
});
