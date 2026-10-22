// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project defaults (parity/specs/project-settings-members.md §"Project
// defaults"; PSET-026): lead and assignee pickers list non-guest members
// plus None, save on change, clear to null, and the guest-visibility switch
// flips the project flag; non-admins see everything disabled; the loaders
// stand in until the project row arrives.
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ProjectDefaultsFields } from "./api.js";
import { ASSIGNEE_FIELD_LABEL, GUEST_VISIBILITY_LABEL, LEAD_FIELD_LABEL, MemberDefaults } from "./MemberDefaults.js";
import { DEFAULTS_NONE_LABEL, type DefaultMemberOption } from "./writes.js";

const ADA = "123e4567-e89b-12d3-a456-42661417400a";
const GRACE = "123e4567-e89b-12d3-a456-42661417400b";

const MEMBERS: DefaultMemberOption[] = [
  { userId: GRACE, label: "grace" },
  { userId: ADA, label: "ada" },
];

function project(overrides: Partial<ProjectDefaultsFields> = {}): ProjectDefaultsFields {
  return {
    project_lead: ADA,
    default_assignee: null,
    guest_view_all_features: false,
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
});

describe("MemberDefaults", () => {
  it("shows loaders until the project row arrives", () => {
    render(<MemberDefaults project={undefined} members={MEMBERS} canEdit pending={false} onSave={vi.fn()} />);
    expect(screen.getByTestId("members-defaults")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: LEAD_FIELD_LABEL })).not.toBeInTheDocument();
  });

  it("paints the saved lead and the empty assignee slot", () => {
    render(<MemberDefaults project={project()} members={MEMBERS} canEdit pending={false} onSave={vi.fn()} />);
    expect(screen.getByRole("button", { name: LEAD_FIELD_LABEL })).toHaveTextContent("ada");
    expect(screen.getByRole("button", { name: ASSIGNEE_FIELD_LABEL })).toHaveTextContent(DEFAULTS_NONE_LABEL);
    expect(screen.getByRole("switch", { name: GUEST_VISIBILITY_LABEL })).not.toBeChecked();
  });

  it("offers non-guest members sorted by name with None first, and saves the pick", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<MemberDefaults project={project()} members={MEMBERS} canEdit pending={false} onSave={onSave} />);
    await user.click(screen.getByRole("button", { name: ASSIGNEE_FIELD_LABEL }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent)).toEqual([
      DEFAULTS_NONE_LABEL,
      "ada",
      "grace",
    ]);
    await user.click(screen.getByRole("option", { name: "grace" }));
    expect(onSave).toHaveBeenCalledWith({ default_assignee: GRACE });
  });

  it("clears a slot to null through the None choice", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<MemberDefaults project={project()} members={MEMBERS} canEdit pending={false} onSave={onSave} />);
    await user.click(screen.getByRole("button", { name: LEAD_FIELD_LABEL }));
    await user.click(await screen.findByRole("option", { name: DEFAULTS_NONE_LABEL }));
    expect(onSave).toHaveBeenCalledWith({ project_lead: null });
  });

  it("flips the guest-visibility flag", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    render(<MemberDefaults project={project()} members={MEMBERS} canEdit pending={false} onSave={onSave} />);
    await user.click(screen.getByRole("switch", { name: GUEST_VISIBILITY_LABEL }));
    expect(onSave).toHaveBeenCalledWith({ guest_view_all_features: true });
  });

  it("shows the saved on state for guest visibility", () => {
    render(
      <MemberDefaults
        project={project({ guest_view_all_features: true })}
        members={MEMBERS}
        canEdit
        pending={false}
        onSave={vi.fn()}
      />
    );
    expect(screen.getByRole("switch", { name: GUEST_VISIBILITY_LABEL })).toBeChecked();
  });

  it("disables every control for non-admins", () => {
    render(<MemberDefaults project={project()} members={MEMBERS} canEdit={false} pending={false} onSave={vi.fn()} />);
    expect(screen.getByRole("button", { name: LEAD_FIELD_LABEL })).toBeDisabled();
    expect(screen.getByRole("button", { name: ASSIGNEE_FIELD_LABEL })).toBeDisabled();
    expect(screen.getByRole("switch", { name: GUEST_VISIBILITY_LABEL })).toHaveAttribute("data-disabled");
  });

  it("disables every control while a save flies", () => {
    render(<MemberDefaults project={project()} members={MEMBERS} canEdit pending onSave={vi.fn()} />);
    expect(screen.getByRole("button", { name: LEAD_FIELD_LABEL })).toBeDisabled();
    expect(screen.getByRole("switch", { name: GUEST_VISIBILITY_LABEL })).toHaveAttribute("data-disabled");
  });

  it("reads an assignee that is not among the offered members as None", () => {
    render(
      <MemberDefaults
        project={project({ default_assignee: "123e4567-e89b-12d3-a456-4266141740ff" })}
        members={MEMBERS}
        canEdit
        pending={false}
        onSave={vi.fn()}
      />
    );
    expect(screen.getByRole("button", { name: ASSIGNEE_FIELD_LABEL })).toHaveTextContent(DEFAULTS_NONE_LABEL);
  });

  it("paints a label-less picker when no member is offered", async () => {
    const user = userEvent.setup();
    render(
      <MemberDefaults project={project({ project_lead: null })} members={[]} canEdit pending={false} onSave={vi.fn()} />
    );
    const picker = screen.getByRole("button", { name: LEAD_FIELD_LABEL });
    expect(picker).toHaveTextContent(DEFAULTS_NONE_LABEL);
    await user.click(picker);
    expect((await screen.findAllByRole("option")).map((option) => option.textContent)).toEqual([DEFAULTS_NONE_LABEL]);
    expect(within(await screen.findByRole("listbox", { name: LEAD_FIELD_LABEL })).getAllByRole("option")).toHaveLength(
      1
    );
    await waitFor(() => expect(screen.getByRole("option", { name: DEFAULTS_NONE_LABEL })).toBeInTheDocument());
  });
});
