// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Button } from "@pidash/kit";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { FieldPicker, fieldPickerOptions } from "./FieldPicker.js";
import { richFilterFields } from "./expression.js";

describe("fieldPickerOptions", () => {
  it("lists only unused fields in catalogue order", () => {
    const options = fieldPickerOptions(richFilterFields(), new Set(["state_group"]));
    expect(options.map((option) => option.id)).toEqual([
      "state_id",
      "priority",
      "assignee_id",
      "created_by_id",
      "mention_id",
      "label_id",
    ]);
  });

  it("returns one disabled exhaustion row when every field is used", () => {
    const options = fieldPickerOptions(
      richFilterFields(),
      new Set(["state_id", "state_group", "priority", "assignee_id", "created_by_id", "mention_id", "label_id"])
    );
    expect(options).toEqual([{ id: "__exhausted__", label: "All filters applied", disabled: true }]);
  });
});

describe("FieldPicker", () => {
  it("adds the picked field and closes", async () => {
    const user = userEvent.setup();
    const onPick = vi.fn();
    render(
      <FieldPicker fields={richFilterFields()} used={new Set()} onPick={onPick} trigger={<Button>Add filter</Button>} />
    );
    await user.click(screen.getByRole("button", { name: "Add filter" }));
    const list = await screen.findByRole("listbox", { name: "Filter properties" });
    await user.click(within(list).getByRole("option", { name: "Priority" }));
    expect(onPick).toHaveBeenCalledWith(expect.objectContaining({ stem: "priority" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("searches the available fields", async () => {
    const user = userEvent.setup();
    render(
      <FieldPicker fields={richFilterFields()} used={new Set()} onPick={() => {}} trigger={<Button>Add</Button>} />
    );
    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.type(await screen.findByRole("combobox", { name: "Search properties" }), "prio");
    expect(
      within(screen.getByRole("listbox"))
        .getAllByRole("option")
        .map((row) => row.textContent)
    ).toEqual(["Priority"]);
  });

  it("renders the exhaustion row disabled and never picks it", async () => {
    const user = userEvent.setup();
    const onPick = vi.fn();
    render(
      <FieldPicker
        fields={richFilterFields()}
        used={
          new Set(["state_id", "state_group", "priority", "assignee_id", "created_by_id", "mention_id", "label_id"])
        }
        onPick={onPick}
        trigger={<Button>Add</Button>}
      />
    );
    await user.click(screen.getByRole("button", { name: "Add" }));
    const exhausted = await screen.findByRole("option", { name: "All filters applied" });
    expect(exhausted).toBeDisabled();
    await user.click(exhausted);
    expect(onPick).not.toHaveBeenCalled();
  });
});
