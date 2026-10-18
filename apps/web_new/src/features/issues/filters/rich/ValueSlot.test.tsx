// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { createAppClient } from "../../../../core/api/client.js";

import { pickerStatesQueryOptions } from "../../../../shared/pickers/index.js";
import { findField, richFilterFields, type RichCondition } from "./expression.js";
import { ValueSlot, selectedValues } from "./ValueSlot.js";

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
