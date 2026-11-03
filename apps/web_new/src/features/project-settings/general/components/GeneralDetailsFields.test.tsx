// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Identity fields (PSET-005/PSET-006): the description placeholder, the
// live identifier hint, visibility/timezone pickers, and the disabled state
// the non-admin form uses. Every edit is reported as a form patch.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { generalFormFrom, type GeneralFormValues } from "../rules.js";
import type { ProjectSettingsDetail } from "@pidash/api-client";
import { GeneralDetailsFields } from "./GeneralDetailsFields.js";

function stored(overrides: Partial<ProjectSettingsDetail> = {}): ProjectSettingsDetail {
  return {
    name: "Website relaunch",
    identifier: "WEB",
    description: "",
    network: 0,
    timezone: "Europe/Lisbon",
    base_branch: "develop",
    repo_url: "",
    logo_props: {},
    ...overrides,
  } as ProjectSettingsDetail;
}

const ZONES = [{ id: "Europe/Lisbon", label: "Lisbon", searchText: "europe/lisbon lisbon" }];

function setup(
  overrides: Partial<Parameters<typeof GeneralDetailsFields>[0]> & { form?: Partial<GeneralFormValues> } = {}
) {
  const { form, ...rest } = overrides;
  const onChange = vi.fn();
  const { unmount } = render(
    <GeneralDetailsFields
      values={{ ...generalFormFrom(stored()), ...form }}
      errors={{}}
      disabled={false}
      checkingIdentifier={false}
      timezones={ZONES}
      timezonesLoading={false}
      timezonesFailed={false}
      onChange={onChange}
      {...rest}
    />
  );
  return { onChange, unmount };
}

describe("GeneralDetailsFields", () => {
  it("shows the description guidance placeholder and the length hint", () => {
    setup();
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveAttribute(
      "placeholder",
      "Project Context (Will be shown to AI agents)"
    );
    expect(screen.getByText("Up to 10 letters and numbers")).toBeInTheDocument();
  });

  it("swaps the hint for the availability check while it runs", () => {
    setup({ checkingIdentifier: true });
    expect(screen.getByText("Checking availability…")).toBeInTheDocument();
  });

  it("reports each text edit as a patch", () => {
    const { onChange } = setup();
    fireEvent.change(screen.getByRole("textbox", { name: "Project name" }), { target: { value: "Renamed" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Description" }), { target: { value: "Notes" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Base branch" }), { target: { value: "trunk" } });
    expect(onChange).toHaveBeenCalledWith({ name: "Renamed" });
    expect(onChange).toHaveBeenCalledWith({ description: "Notes" });
    expect(onChange).toHaveBeenCalledWith({ baseBranch: "trunk" });
  });

  it("picks visibility and timezone", async () => {
    const user = userEvent.setup();
    const { onChange } = setup();
    await user.click(screen.getByTestId("general-visibility-trigger"));
    await user.click(screen.getByRole("option", { name: "Public" }));
    expect(onChange).toHaveBeenCalledWith({ network: 2 });
    await user.click(screen.getByTestId("general-timezone-trigger"));
    await user.click(screen.getByRole("option", { name: /Lisbon/ }));
    expect(onChange).toHaveBeenCalledWith({ timezone: "Europe/Lisbon" });
  });

  it("falls back to private for an unknown network", () => {
    setup({ form: { network: 7 } });
    expect(screen.getByTestId("general-visibility")).toHaveTextContent("Private");
  });

  it("disables the timezone picker when the zone list failed", () => {
    setup({ timezonesFailed: true });
    expect(screen.getByTestId("general-timezone-trigger")).toBeDisabled();
  });

  it("disables every field for a non-admin", () => {
    setup({ disabled: true });
    expect(screen.getByRole("textbox", { name: "Project name" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Short code" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Description" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Base branch" })).toBeDisabled();
    expect(screen.getByTestId("general-visibility-trigger")).toBeDisabled();
    expect(screen.getByTestId("general-timezone-trigger")).toBeDisabled();
  });
});
