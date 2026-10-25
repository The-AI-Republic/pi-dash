// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Finding-handling field (parity/specs/agents.md §4, AGT-018): three modes,
// issue filing first, and a help line that follows the selection.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DEFAULT_OUTCOME_MODE, OUTCOME_OPTIONS, OutcomeModeField, outcomeHelp } from "./OutcomeModeField.js";

function Harness({ onChange }: { onChange: (mode: "create_issue" | "apply_fix" | "fix_and_review") => void }) {
  const [mode, setMode] = React.useState(DEFAULT_OUTCOME_MODE);
  return (
    <OutcomeModeField
      value={mode}
      onChange={(next) => {
        setMode(next);
        onChange(next);
      }}
    />
  );
}

describe("OutcomeModeField", () => {
  it("offers the three modes in order with issues checked by default", () => {
    render(<OutcomeModeField value={DEFAULT_OUTCOME_MODE} onChange={vi.fn()} />);
    const radios = screen.getAllByRole("radio");
    expect(radios.map((radio) => radio.textContent)).toEqual(["Create issues", "Apply fix", "Fix & open for review"]);
    expect(screen.getByRole("radio", { name: "Create issues" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "Apply fix" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(outcomeHelp("create_issue"))).toHaveTextContent("open issue");
  });

  it("keeps exactly one option in the tab order", () => {
    render(<OutcomeModeField value="apply_fix" onChange={vi.fn()} />);
    expect(screen.getByRole("radio", { name: "Apply fix" })).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("radio", { name: "Create issues" })).toHaveAttribute("tabindex", "-1");
    expect(screen.getByRole("radio", { name: "Fix & open for review" })).toHaveAttribute("tabindex", "-1");
  });

  it("moves the choice and focus with the arrow keys", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    screen.getByRole("radio", { name: "Create issues" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("apply_fix");
    expect(screen.getByRole("radio", { name: "Apply fix" })).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(onChange).toHaveBeenLastCalledWith("create_issue");
    expect(screen.getByRole("radio", { name: "Create issues" })).toHaveFocus();
  });

  it("wraps around at the end of the group", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    screen.getByRole("radio", { name: "Create issues" }).focus();
    await user.keyboard("{ArrowUp}");
    expect(onChange).toHaveBeenLastCalledWith("fix_and_review");
  });

  it("reports a new choice and moves the help line with it", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    await user.click(screen.getByRole("radio", { name: "Apply fix" }));
    expect(onChange).toHaveBeenLastCalledWith("apply_fix");
    expect(screen.getByText(outcomeHelp("apply_fix"))).toHaveTextContent("pull request");
    await user.click(screen.getByRole("radio", { name: "Fix & open for review" }));
    expect(onChange).toHaveBeenLastCalledWith("fix_and_review");
    expect(screen.getByText(outcomeHelp("fix_and_review"))).toHaveTextContent("In Progress");
  });

  it("disables every option while busy", () => {
    render(<OutcomeModeField value="apply_fix" disabled onChange={vi.fn()} />);
    for (const radio of screen.getAllByRole("radio")) expect(radio).toBeDisabled();
    expect(screen.getByRole("radio", { name: "Apply fix" })).toHaveAttribute("aria-checked", "true");
  });

  it("names every mode and every mode has help text", () => {
    expect(OUTCOME_OPTIONS.map((option) => option.value)).toEqual(["create_issue", "apply_fix", "fix_and_review"]);
    for (const option of OUTCOME_OPTIONS) expect(outcomeHelp(option.value).length).toBeGreaterThan(20);
  });
});
