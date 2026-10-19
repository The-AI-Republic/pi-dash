// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preference picker (parity/specs/profile.md §7, PROF-016/017): a button
// carrying the stored label opens the kit list, searching narrows it and
// a pick reports the stored value back to the row.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PICKER_SEARCH_LABEL, PreferencePicker } from "./PreferencePicker.js";

const OPTIONS = [
  { value: 0, label: "Sunday" },
  { value: 1, label: "Monday" },
  { value: 2, label: "Tuesday" },
];

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

describe("PreferencePicker", () => {
  it("shows the stored label on the trigger and marks the selected option", async () => {
    render(
      <PreferencePicker
        label="First day of the week"
        valueLabel="Monday"
        value={1}
        options={OPTIONS}
        onSelect={() => undefined}
      />
    );
    const trigger = screen.getByRole("button", { name: "First day of the week" });
    expect(trigger).toHaveTextContent("Monday");
    fireEvent.click(trigger);
    expect(await screen.findByRole("option", { name: "Monday" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("option", { name: "Sunday" })).toHaveAttribute("aria-selected", "false");
  });

  it("reports the picked value and closes", async () => {
    const onSelect = vi.fn();
    render(
      <PreferencePicker
        label="First day of the week"
        valueLabel="Monday"
        value={1}
        options={OPTIONS}
        onSelect={onSelect}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "First day of the week" }));
    fireEvent.click(await screen.findByRole("option", { name: "Tuesday" }));
    expect(onSelect).toHaveBeenCalledWith(2);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("narrows the list by search text, including caller extras", async () => {
    const user = userEvent.setup();
    render(
      <PreferencePicker
        label="Timezone"
        valueLabel="Not set"
        value=""
        options={[
          { value: "America/Los_Angeles", label: "Los Angeles" },
          { value: "Europe/Paris", label: "Paris" },
        ]}
        onSelect={() => undefined}
        searchText={(option) => `${option.label} ${option.value}`}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Timezone" }));
    await user.type(screen.getByRole("combobox", { name: PICKER_SEARCH_LABEL }), "paris");
    expect(optionNames()).toEqual(["Paris"]);
    await user.clear(screen.getByRole("combobox", { name: PICKER_SEARCH_LABEL }));
    await user.type(screen.getByRole("combobox", { name: PICKER_SEARCH_LABEL }), "america");
    expect(optionNames()).toEqual(["Los Angeles"]);
    await user.clear(screen.getByRole("combobox", { name: PICKER_SEARCH_LABEL }));
    await user.type(screen.getByRole("combobox", { name: PICKER_SEARCH_LABEL }), "zzz");
    expect(screen.getByText("No matching options.")).toBeInTheDocument();
  });

  it("blocks opening while disabled", () => {
    render(
      <PreferencePicker
        label="Theme"
        valueLabel="Dark"
        value="dark"
        options={[{ value: "dark", label: "Dark" }]}
        onSelect={() => undefined}
        disabled
      />
    );
    expect(screen.getByRole("button", { name: "Theme" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Theme" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });
});
