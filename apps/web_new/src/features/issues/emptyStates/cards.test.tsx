// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The seven context cards (parity/specs/issues.md §8, ISS-068–074): each
// pins its copy, the entries it may carry and their guest state, and the
// card's own context stamp.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  ArchivedIssuesEmpty,
  CycleIssuesEmpty,
  FilteredEmptyCard,
  GlobalIssuesEmpty,
  ModuleIssuesEmpty,
  NO_MATCH_DESCRIPTION,
  NO_MATCH_TITLE,
  ProfileIssuesEmpty,
  ProjectIssuesEmpty,
  SavedViewIssuesEmpty,
} from "./cards.js";

function card(): HTMLElement {
  return screen.getByTestId("issues-empty-state");
}

function actionLabels(root: HTMLElement): string[] {
  return within(root)
    .queryAllByRole("button")
    .map((button) => button.textContent?.trim() ?? "");
}

describe("FilteredEmptyCard", () => {
  it("names the search that matched nothing and offers clearing", () => {
    const onClear = vi.fn();
    render(<FilteredEmptyCard context="cycle" clearLabel="Clear filters" canAct onClear={onClear} />);
    expect(card()).toHaveAttribute("data-context", "cycle-filtered");
    expect(screen.getByText(NO_MATCH_TITLE)).toBeInTheDocument();
    expect(screen.getByText(NO_MATCH_DESCRIPTION)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("disables clearing for a guest", () => {
    render(<FilteredEmptyCard context="project" clearLabel="Clear all filters" canAct={false} onClear={() => {}} />);
    expect(screen.getByRole("button", { name: "Clear all filters" })).toBeDisabled();
  });
});

describe("ProjectIssuesEmpty", () => {
  it("asks for the first work item and opens creation", () => {
    const onCreate = vi.fn();
    render(<ProjectIssuesEmpty canAct onCreate={onCreate} />);
    expect(card()).toHaveAttribute("data-context", "project");
    expect(screen.getByText("Start with your first work item.")).toBeInTheDocument();
    const entry = screen.getByRole("button", { name: "Create your first work item" });
    expect(entry).toBeEnabled();
    fireEvent.click(entry);
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("keeps the entry dead for a guest", () => {
    render(<ProjectIssuesEmpty canAct={false} onCreate={() => {}} />);
    expect(screen.getByRole("button", { name: "Create your first work item" })).toBeDisabled();
  });
});

describe("CycleIssuesEmpty", () => {
  it("offers both entries for an open cycle", () => {
    const onCreate = vi.fn();
    const onAddExisting = vi.fn();
    render(<CycleIssuesEmpty completed={false} canAct onCreate={onCreate} onAddExisting={onAddExisting} />);
    expect(card()).toHaveAttribute("data-context", "cycle");
    expect(screen.getByText("No work items to show in this cycle")).toBeInTheDocument();
    expect(actionLabels(card())).toEqual(["Create work item", "Add existing work item"]);
    fireEvent.click(screen.getByRole("button", { name: "Create work item" }));
    fireEvent.click(screen.getByRole("button", { name: "Add existing work item" }));
    expect(onCreate).toHaveBeenCalledTimes(1);
    expect(onAddExisting).toHaveBeenCalledTimes(1);
  });

  it("states a completed cycle as a record with no entries", () => {
    render(<CycleIssuesEmpty completed canAct onCreate={() => {}} onAddExisting={() => {}} />);
    expect(card()).toHaveAttribute("data-context", "cycle-completed");
    expect(screen.getByText("No work items in the cycle")).toBeInTheDocument();
    expect(actionLabels(card())).toEqual([]);
  });

  it("disables both entries for a guest", () => {
    render(<CycleIssuesEmpty completed={false} canAct={false} onCreate={() => {}} onAddExisting={() => {}} />);
    expect(screen.getByRole("button", { name: "Create work item" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add existing work item" })).toBeDisabled();
  });
});

describe("ModuleIssuesEmpty", () => {
  it("offers creation and attaching existing rows", () => {
    const onCreate = vi.fn();
    const onAddExisting = vi.fn();
    render(<ModuleIssuesEmpty canAct onCreate={onCreate} onAddExisting={onAddExisting} />);
    expect(card()).toHaveAttribute("data-context", "module");
    expect(screen.getByText("No work items to show in this Module")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add existing work item" }));
    expect(onAddExisting).toHaveBeenCalledTimes(1);
    expect(onCreate).not.toHaveBeenCalled();
  });

  it("disables both entries for a guest", () => {
    render(<ModuleIssuesEmpty canAct={false} onCreate={() => {}} onAddExisting={() => {}} />);
    expect(screen.getByRole("button", { name: "Create work item" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add existing work item" })).toBeDisabled();
  });
});

describe("ArchivedIssuesEmpty", () => {
  it("points at the automation rule instead of creation", () => {
    const onSetAutomation = vi.fn();
    render(<ArchivedIssuesEmpty canAct onSetAutomation={onSetAutomation} />);
    expect(card()).toHaveAttribute("data-context", "archived");
    expect(screen.getByText("No archived work items yet")).toBeInTheDocument();
    expect(actionLabels(card())).toEqual(["Set automation"]);
    fireEvent.click(screen.getByRole("button", { name: "Set automation" }));
    expect(onSetAutomation).toHaveBeenCalledTimes(1);
  });

  it("disables the automation entry for a guest", () => {
    render(<ArchivedIssuesEmpty canAct={false} onSetAutomation={() => {}} />);
    expect(screen.getByRole("button", { name: "Set automation" })).toBeDisabled();
  });
});

describe("SavedViewIssuesEmpty", () => {
  it("describes the view and opens the create dialog", () => {
    const onCreate = vi.fn();
    render(<SavedViewIssuesEmpty canAct onCreate={onCreate} />);
    expect(card()).toHaveAttribute("data-context", "view");
    expect(screen.getByText("View work items will appear here")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "New work item" }));
    expect(onCreate).toHaveBeenCalledTimes(1);
  });

  it("keeps the entry dead when the caller cannot create", () => {
    render(<SavedViewIssuesEmpty canAct={false} onCreate={() => {}} />);
    expect(screen.getByRole("button", { name: "New work item" })).toBeDisabled();
  });
});

describe("GlobalIssuesEmpty", () => {
  it("asks for a project when the workspace shows none", () => {
    const onCreateProject = vi.fn();
    render(<GlobalIssuesEmpty projectCount={0} canAct onCreateProject={onCreateProject} onCreateWorkItem={() => {}} />);
    expect(card()).toHaveAttribute("data-context", "global-no-project");
    expect(screen.getByText("No project")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start your first project" }));
    expect(onCreateProject).toHaveBeenCalledTimes(1);
  });

  it("asks for a work item once a project exists", () => {
    const onCreateWorkItem = vi.fn();
    render(
      <GlobalIssuesEmpty projectCount={2} canAct onCreateProject={() => {}} onCreateWorkItem={onCreateWorkItem} />
    );
    expect(card()).toHaveAttribute("data-context", "global");
    expect(screen.getByText("No Views yet")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    expect(onCreateWorkItem).toHaveBeenCalledTimes(1);
  });

  it("disables either entry for a guest", () => {
    const project = render(
      <GlobalIssuesEmpty projectCount={0} canAct={false} onCreateProject={() => {}} onCreateWorkItem={() => {}} />
    );
    expect(screen.getByRole("button", { name: "Start your first project" })).toBeDisabled();
    project.unmount();
    render(
      <GlobalIssuesEmpty projectCount={1} canAct={false} onCreateProject={() => {}} onCreateWorkItem={() => {}} />
    );
    expect(screen.getByRole("button", { name: "Add work item" })).toBeDisabled();
  });
});

describe("ProfileIssuesEmpty", () => {
  it("names the assigned tab without an entry", () => {
    render(<ProfileIssuesEmpty tab="assigned" />);
    expect(card()).toHaveAttribute("data-context", "profile-assigned");
    expect(screen.getByText("No work items are assigned to you")).toBeInTheDocument();
    expect(actionLabels(card())).toEqual([]);
  });

  it("names the created tab without an entry", () => {
    render(<ProfileIssuesEmpty tab="created" />);
    expect(card()).toHaveAttribute("data-context", "profile-created");
    expect(screen.getByText("No work items yet")).toBeInTheDocument();
    expect(actionLabels(card())).toEqual([]);
  });

  it("names the subscribed tab without an entry", () => {
    render(<ProfileIssuesEmpty tab="subscribed" />);
    expect(card()).toHaveAttribute("data-context", "profile-subscribed");
    expect(screen.getByText("No work items yet")).toBeInTheDocument();
    expect(screen.getByText(/Subscribe to work items/)).toBeInTheDocument();
    expect(actionLabels(card())).toEqual([]);
  });
});
