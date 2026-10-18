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

import { ConditionPill } from "./ConditionPill.js";
import { findField, richFilterFields, type RichCondition } from "./expression.js";

const FIELDS = richFilterFields();
const PRIORITY = findField(FIELDS, "priority")!;

function wrap(node: React.ReactElement): React.ReactElement {
  return wrapInner(node);
}

function wrapInner(node: React.ReactElement): React.ReactElement {
  createAppClient("https://api.test", (() => {
    throw new Error("unexpected network fetch in a seeded render");
  }) as never);
  return (
    <QueryClientProvider client={new QueryClient()}>
      <React.Fragment>{node}</React.Fragment>
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

function seed(overrides: Partial<RichCondition> = {}): RichCondition {
  return { key: "a", property: "priority", operator: "in", negated: false, value: ["urgent"], ...overrides };
}

function renderPill(condition: RichCondition, onChange = vi.fn(), onRemove = vi.fn(), conditions = [condition]) {
  render(
    wrap(
      <ConditionPill
        workspaceSlug="acme"
        projectId="123e4567-e89b-12d3-a456-426614174000"
        fields={FIELDS}
        field={findField(FIELDS, condition.property)}
        condition={condition}
        conditions={conditions}
        used={new Set(conditions.map((row) => row.property))}
        onChange={onChange}
        onRemove={onRemove}
      />
    )
  );
  return { onChange, onRemove };
}

describe("ConditionPill", () => {
  it("shows the property, operator and value and removes itself", async () => {
    const user = userEvent.setup();
    const { onRemove } = renderPill(seed());
    const pill = screen.getByTestId("rich-condition");
    expect(within(pill).getByRole("button", { name: /Priority/ })).toBeInTheDocument();
    expect(within(pill).getByRole("button", { name: /is/ })).toBeInTheDocument();
    expect(within(pill).getByRole("button", { name: /Urgent/ })).toBeInTheDocument();
    await user.click(within(pill).getByRole("button", { name: "Remove filter" }));
    expect(onRemove).toHaveBeenCalledTimes(1);
  });

  it("offers the negated operator and stores the negation flag", async () => {
    const user = userEvent.setup();
    const { onChange } = renderPill(seed());
    await user.click(screen.getByRole("button", { name: /is$/ }));
    await user.click(await screen.findByRole("option", { name: "is not" }));
    expect(onChange).toHaveBeenCalledWith({ ...seed(), negated: true });
  });

  it("re-picks the property and resets operator and value to the field default", async () => {
    const user = userEvent.setup();
    const { onChange } = renderPill(seed());
    await user.click(screen.getByRole("button", { name: /Priority/ }));
    const list = await screen.findByRole("listbox", { name: "Filter properties" });
    await user.click(within(list).getByRole("option", { name: "State group" }));
    expect(onChange).toHaveBeenCalledWith({
      key: "a",
      property: "state_group",
      operator: "in",
      negated: false,
      value: null,
    });
  });

  it("locks the operator control for a field without a negated form", () => {
    const locked = { ...PRIORITY, allowNegative: false };
    render(
      wrap(
        <ConditionPill
          workspaceSlug="acme"
          projectId="123e4567-e89b-12d3-a456-426614174000"
          fields={[locked]}
          field={locked}
          condition={seed()}
          conditions={[seed()]}
          used={new Set(["priority"])}
          onChange={vi.fn()}
          onRemove={vi.fn()}
        />
      )
    );
    expect(screen.getByRole("button", { name: /is$/ })).toBeDisabled();
  });

  it("renders an unknown property as an error pill with only remove", async () => {
    const user = userEvent.setup();
    const onRemove = vi.fn();
    render(
      wrap(
        <ConditionPill
          workspaceSlug="acme"
          projectId="123e4567-e89b-12d3-a456-426614174000"
          fields={FIELDS}
          field={null}
          condition={seed({ property: "deleted_field", value: ["x"] })}
          conditions={[seed({ property: "deleted_field", value: ["x"] })]}
          used={new Set()}
          onChange={vi.fn()}
          onRemove={onRemove}
        />
      )
    );
    expect(screen.getByTestId("rich-condition-invalid")).toHaveTextContent("Unavailable filter");
    expect(screen.queryByTestId("rich-condition")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove filter" }));
    expect(onRemove).toHaveBeenCalledTimes(1);
  });
});
