// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition filter dimensions on the sub-issue dropdown (ISS-234): the
// community build renders neither section and reads nothing, a cloud build
// lists what its providers offer, and the applied chips name and remove
// what is set.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { edition } from "../../../core/edition/index.js";
import {
  EditionAppliedChips,
  EditionFilterSections,
  appliedChips,
  type EditionFilterSectionsProps,
} from "./EditionFilterSections.js";
import { emptyEditionFilters, type EditionFilters } from "./filterModel.js";
import { ISSUE_TYPE_FILTER_FLAG, TEAM_PROJECT_FILTER_FLAG } from "./flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";

const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const TEAM = "423e4567-e89b-12d3-a456-426614174003";
const TYPE = "523e4567-e89b-12d3-a456-426614174004";

afterEach(() => {
  cleanup();
  resetEditionIssuesProviders();
  delete edition.flags[TEAM_PROJECT_FILTER_FLAG];
  delete edition.flags[ISSUE_TYPE_FILTER_FLAG];
});

function renderSections(
  overrides: Partial<EditionFilterSectionsProps> = {},
  filters: EditionFilters = emptyEditionFilters()
) {
  const onToggle = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <EditionFilterSections
        workspaceSlug="acme"
        projectId={PROJECT}
        open
        filters={filters}
        onToggle={onToggle}
        {...overrides}
      />
    </QueryClientProvider>
  );
  return { onToggle };
}

describe("EditionFilterSections", () => {
  it("renders neither section in the community build", () => {
    registerEditionIssuesProviders({
      teamProjects: async () => [{ id: TEAM, name: "Platform", identifier: "PLT" }],
      workItemTypes: async () => [{ id: TYPE, name: "Bug" }],
    });
    const teamSource = vi.fn(async () => [{ id: TEAM, name: "Platform", identifier: "PLT" }]);
    registerEditionIssuesProviders({ teamProjects: teamSource });
    renderSections();
    expect(screen.queryByTestId("sub-issue-filter-team-project")).toBeNull();
    expect(screen.queryByTestId("sub-issue-filter-type")).toBeNull();
    expect(screen.queryByText(/team/i)).toBeNull();
    expect(teamSource).not.toHaveBeenCalled();
  });

  it("lists the provider's team projects when its flag is on", async () => {
    edition.flags[TEAM_PROJECT_FILTER_FLAG] = true;
    registerEditionIssuesProviders({
      teamProjects: async () => [{ id: TEAM, name: "Platform", identifier: "PLT" }],
    });
    const { onToggle } = renderSections(undefined, { teamProject: [TEAM], issueType: [] });
    const section = await screen.findByTestId("sub-issue-filter-team-project");
    expect(await screen.findByRole("checkbox", { name: "Platform" })).toHaveAttribute("aria-checked", "true");
    expect(section).toHaveTextContent("Team projects (1)");
    await userEvent.setup().click(screen.getByRole("checkbox", { name: "Platform" }));
    expect(onToggle).toHaveBeenCalledWith("teamProject", TEAM);
  });

  it("searches team projects by their short code too", async () => {
    edition.flags[TEAM_PROJECT_FILTER_FLAG] = true;
    registerEditionIssuesProviders({
      teamProjects: async () => [{ id: TEAM, name: "Platform", identifier: "PLT" }],
    });
    renderSections();
    await screen.findByRole("checkbox", { name: "Platform" });
    await userEvent.setup().type(screen.getByRole("searchbox", { name: "Search Team projects" }), "plt");
    expect(screen.getByRole("checkbox", { name: "Platform" })).toBeInTheDocument();
  });

  it("lists the provider's work-item types when its flag is on", async () => {
    edition.flags[ISSUE_TYPE_FILTER_FLAG] = true;
    registerEditionIssuesProviders({ workItemTypes: async () => [{ id: TYPE, name: "Bug" }] });
    const { onToggle } = renderSections();
    expect(await screen.findByRole("checkbox", { name: "Bug" })).toHaveAttribute("aria-checked", "false");
    await userEvent.setup().click(screen.getByRole("checkbox", { name: "Bug" }));
    expect(onToggle).toHaveBeenCalledWith("issueType", TYPE);
  });

  it("shows a placeholder while the rows load and the failure in place of them", async () => {
    edition.flags[ISSUE_TYPE_FILTER_FLAG] = true;
    registerEditionIssuesProviders({ workItemTypes: () => new Promise(() => undefined) });
    renderSections();
    expect(await screen.findByText("Loading types")).toBeInTheDocument();
    cleanup();
    registerEditionIssuesProviders({
      workItemTypes: async () => {
        throw new Error("down");
      },
    });
    renderSections();
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load types.");
  });

  it("names a failed team-project read in place of its rows", async () => {
    edition.flags[TEAM_PROJECT_FILTER_FLAG] = true;
    registerEditionIssuesProviders({
      teamProjects: async () => {
        throw new Error("down");
      },
    });
    renderSections();
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load team projects.");
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("keeps reads idle while the dropdown is closed", async () => {
    edition.flags[TEAM_PROJECT_FILTER_FLAG] = true;
    const source = vi.fn(async () => [{ id: TEAM, name: "Platform", identifier: "PLT" }]);
    registerEditionIssuesProviders({ teamProjects: source });
    renderSections({ open: false });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(source).not.toHaveBeenCalled();
  });
});

describe("EditionAppliedChips", () => {
  it("renders nothing with no applied value", () => {
    render(<EditionAppliedChips chips={[]} onRemove={vi.fn()} />);
    expect(screen.queryByTestId("edition-filter-chips")).toBeNull();
  });

  it("names each applied value and removes the picked one", async () => {
    const onRemove = vi.fn();
    const chips = appliedChips(
      { teamProject: [TEAM], issueType: [TYPE] },
      {
        teamProject: [{ id: TEAM, name: "Platform", identifier: "PLT" }],
        issueType: [{ id: TYPE, name: "Bug" }],
      }
    );
    expect(chips).toEqual([
      { key: "teamProject", value: TEAM, label: "Platform" },
      { key: "issueType", value: TYPE, label: "Bug" },
    ]);
    render(<EditionAppliedChips chips={chips} onRemove={onRemove} />);
    expect(screen.getByTestId("edition-filter-chips")).toHaveTextContent("Platform");
    await userEvent.setup().click(screen.getByRole("button", { name: "Remove Bug filter" }));
    expect(onRemove).toHaveBeenCalledWith({ key: "issueType", value: TYPE, label: "Bug" });
  });

  it("falls back to the raw id when the option list has not loaded", () => {
    const chips = appliedChips({ teamProject: [TEAM], issueType: [] }, { teamProject: [], issueType: [] });
    expect(chips[0]?.label).toBe(TEAM);
  });

  it("waits for the rows once the query resolves", async () => {
    edition.flags[TEAM_PROJECT_FILTER_FLAG] = true;
    registerEditionIssuesProviders({
      teamProjects: async () => [{ id: TEAM, name: "Platform", identifier: "PLT" }],
    });
    renderSections();
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Platform" })).toBeInTheDocument());
  });
});
