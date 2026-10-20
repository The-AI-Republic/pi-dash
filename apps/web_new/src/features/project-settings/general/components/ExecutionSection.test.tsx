// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution section (PSET-009): the cadence choice list, the label of an
// unknown stored executor, the inert policy while ticking is off, and the
// budget/no-cap edits the form receives.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CADENCE_OPTIONS, CUSTOM_CADENCE_VALUE, generalFormFrom, type GeneralFormValues } from "../rules.js";
import type { ProjectSettingsDetail } from "@pidash/api-client";
import { cadenceChoices, ExecutionSection } from "./ExecutionSection.js";

function stored(overrides: Partial<ProjectSettingsDetail> = {}): ProjectSettingsDetail {
  return {
    name: "Website relaunch",
    identifier: "WEB",
    description: "",
    network: 0,
    timezone: "UTC",
    base_branch: "main",
    repo_url: "",
    logo_props: {},
    default_agent_executor: "local_runner",
    agent_ticking_enabled: true,
    agent_default_max_ticks: 6,
    agent_default_interval_seconds: 10_800,
    agent_review_default_interval_seconds: 10_800,
    agent_test_default_interval_seconds: 10_800,
    ...overrides,
  } as ProjectSettingsDetail;
}

function setup(
  overrides: Partial<Parameters<typeof ExecutionSection>[0]> & { form?: Partial<GeneralFormValues> } = {}
) {
  const { form, ...rest } = overrides;
  const onChange = vi.fn();
  render(
    <ExecutionSection
      values={{ ...generalFormFrom(stored()), ...form }}
      errors={{}}
      disabled={false}
      executorOptions={[{ kind: "local_runner", available: true }]}
      onChange={onChange}
      {...rest}
    />
  );
  return { onChange };
}

describe("cadenceChoices", () => {
  it("lists the presets and adds the custom marker only when it applies", () => {
    expect(cadenceChoices("10800")).toEqual(
      CADENCE_OPTIONS.map((option) => ({ id: String(option.seconds), label: option.label }))
    );
    expect(cadenceChoices(CUSTOM_CADENCE_VALUE).at(-1)).toEqual({ id: CUSTOM_CADENCE_VALUE, label: "Custom" });
  });
});

describe("ExecutionSection", () => {
  it("shows an unknown stored executor by its stored kind", () => {
    setup({ form: { executor: "future_runner" } });
    expect(screen.getByTestId("general-executor")).toHaveTextContent("future_runner");
  });

  it("leaves the budget and cadence inert while ticking is off without disabling the switch", () => {
    setup({ form: { ticking: false } });
    expect(screen.getByRole("spinbutton", { name: "Run budget per work item" })).toBeDisabled();
    expect(screen.getByTestId("general-cadence-trigger")).toBeDisabled();
    expect(screen.getByTestId("general-ticking")).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("general-ticking")).not.toHaveAttribute("aria-disabled", "true");
  });

  it("passes a typed budget with the remembered cap and no-cap off", () => {
    const { onChange } = setup();
    const budget = screen.getByRole("spinbutton", { name: "Run budget per work item" });
    fireEvent.change(budget, { target: { value: "12" } });
    expect(onChange).toHaveBeenLastCalledWith({ budget: "12", lastCappedBudget: "12", noCap: false });
  });

  it("keeps the last cap when the budget is cleared", () => {
    const { onChange } = setup();
    fireEvent.change(screen.getByRole("spinbutton", { name: "Run budget per work item" }), { target: { value: "" } });
    expect(onChange).toHaveBeenLastCalledWith({ budget: "", lastCappedBudget: "6", noCap: false });
  });

  it("remembers the cap across the no-cap toggle", async () => {
    const user = userEvent.setup();
    const { onChange } = setup();
    await user.click(screen.getByTestId("general-nocap"));
    expect(onChange).toHaveBeenCalledWith({ noCap: true, budget: "" });
  });

  it("reports the switch and a picked cadence", async () => {
    const user = userEvent.setup();
    const { onChange } = setup();
    await user.click(screen.getByTestId("general-ticking"));
    expect(onChange).toHaveBeenCalledWith({ ticking: false });
    await user.click(screen.getByTestId("general-cadence-trigger"));
    await user.click(screen.getByRole("option", { name: "30 minutes" }));
    expect(onChange).toHaveBeenCalledWith({ cadence: "1800" });
  });

  it("renders an inline budget error", () => {
    setup({ errors: { budget: "The run budget must be at least one" } });
    expect(screen.getByText("The run budget must be at least one")).toBeInTheDocument();
  });
});
