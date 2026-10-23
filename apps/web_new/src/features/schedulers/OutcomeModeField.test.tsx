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
      idPrefix="om"
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
    render(<OutcomeModeField idPrefix="om" value={DEFAULT_OUTCOME_MODE} onChange={vi.fn()} />);
    const radios = screen.getByRole("radiogroup", { name: "What to do with findings" }).querySelectorAll("input");
    expect(Array.from(radios, (radio) => radio.parentElement?.textContent)).toEqual([
      "Create issues",
      "Apply fix",
      "Fix & open for review",
    ]);
    expect(screen.getByRole("radio", { name: "Create issues" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Apply fix" })).not.toBeChecked();
    expect(screen.getByText(outcomeHelp("create_issue"))).toHaveTextContent("issues");
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
    render(<OutcomeModeField idPrefix="om" value="apply_fix" disabled onChange={vi.fn()} />);
    for (const radio of screen.getAllByRole("radio")) expect(radio).toBeDisabled();
    expect(screen.getByRole("radio", { name: "Apply fix" })).toBeChecked();
  });

  it("names every mode and every mode has help text", () => {
    expect(OUTCOME_OPTIONS.map((option) => option.value)).toEqual(["create_issue", "apply_fix", "fix_and_review"]);
    for (const option of OUTCOME_OPTIONS) expect(outcomeHelp(option.value).length).toBeGreaterThan(20);
  });
});
