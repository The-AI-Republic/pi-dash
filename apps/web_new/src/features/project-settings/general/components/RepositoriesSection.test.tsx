// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository list section (NEWPIDASH-413): row rendering with provider
// labels and the repo-default caption, add/edit/remove commits, the
// client-side rules and the server refusals mapped onto their rows.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { REPO_DUPLICATE_MESSAGE, REPO_URL_REQUIRED_MESSAGE, type RepoRow } from "../rules.js";
import { RepositoriesSection } from "./RepositoriesSection.js";

const ROW: RepoRow = {
  id: "223e4567-e89b-12d3-a456-426614174010",
  url: "https://github.com/acme/site",
  provider: "github",
  baseBranch: "develop",
};

function setup(rows: RepoRow[] = [], commit: (rows: RepoRow[]) => Promise<boolean> = async () => true) {
  const onCommit = vi.fn(commit);
  render(
    <RepositoriesSection
      rows={rows}
      disabled={false}
      saving={false}
      serverErrors={{ rows: new Map(), general: [] }}
      onCommit={onCommit}
    />
  );
  return { onCommit };
}

describe("RepositoriesSection", () => {
  it("renders every row with provider label and branch, or the repo-default caption", () => {
    setup([
      ROW,
      {
        id: "223e4567-e89b-12d3-a456-426614174011",
        url: "git@git.test:acme/other.git",
        provider: "other",
        baseBranch: "",
      },
    ]);
    const rows = screen.getAllByTestId("general-repos-row");
    expect(within(rows[0] as HTMLElement).getByTestId("general-repos-row-url")).toHaveTextContent(
      "https://github.com/acme/site"
    );
    expect(within(rows[0] as HTMLElement).getByTestId("general-repos-row-provider")).toHaveTextContent("GitHub");
    expect(within(rows[0] as HTMLElement).getByTestId("general-repos-row-branch")).toHaveTextContent("develop");
    expect(within(rows[1] as HTMLElement).getByTestId("general-repos-row-provider")).toHaveTextContent("Other");
    expect(within(rows[1] as HTMLElement).getByTestId("general-repos-row-branch")).toHaveTextContent("repo default");
  });

  it("renders the empty state when no row exists", () => {
    setup([]);
    expect(screen.getByTestId("general-repos-empty")).toBeInTheDocument();
  });

  it("adds a row with the inferred provider", async () => {
    const { onCommit } = setup([ROW]);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("general-repos-add"));
    fireEvent.change(screen.getByTestId("general-repos-new-url"), {
      target: { value: "https://gitlab.com/acme/other" },
    });
    fireEvent.change(screen.getByTestId("general-repos-new-branch"), { target: { value: "trunk" } });
    // The provider follows the typed URL until the select is touched.
    expect(screen.getByTestId("general-repos-new-provider-trigger")).toHaveTextContent("GitLab");
    await user.click(screen.getByTestId("general-repos-new-confirm"));
    expect(onCommit).toHaveBeenCalledTimes(1);
    const candidate = onCommit.mock.calls[0]?.[0] ?? [];
    expect(candidate).toHaveLength(2);
    expect(candidate[1]).toMatchObject({
      url: "https://gitlab.com/acme/other",
      provider: "gitlab",
      baseBranch: "trunk",
    });
    expect(typeof candidate[1]?.id).toBe("string");
  });

  it("refuses an empty URL without committing", async () => {
    const { onCommit } = setup([]);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("general-repos-add"));
    await user.click(screen.getByTestId("general-repos-new-confirm"));
    expect(screen.getByTestId("general-repos-new-url")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText(REPO_URL_REQUIRED_MESSAGE)).toBeInTheDocument();
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("rejects a duplicate of another row in another URL form", async () => {
    const { onCommit } = setup([ROW]);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("general-repos-add"));
    fireEvent.change(screen.getByTestId("general-repos-new-url"), {
      target: { value: "git@github.com:acme/site.git" },
    });
    await user.click(screen.getByTestId("general-repos-new-confirm"));
    expect(screen.getByText(REPO_DUPLICATE_MESSAGE)).toBeInTheDocument();
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("keeps an explicit provider pick when the URL changes", async () => {
    setup([]);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("general-repos-add"));
    await user.click(screen.getByTestId("general-repos-new-provider-trigger"));
    await user.click(screen.getByRole("option", { name: "Other" }));
    fireEvent.change(screen.getByTestId("general-repos-new-url"), {
      target: { value: "https://github.com/acme/site" },
    });
    expect(screen.getByTestId("general-repos-new-provider-trigger")).toHaveTextContent("Other");
  });

  it("edits a row and commits the whole list", async () => {
    const { onCommit } = setup([ROW]);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("general-repos-edit"));
    const url = screen.getByTestId("general-repos-edit-url");
    fireEvent.change(url, { target: { value: "https://github.com/acme/renamed" } });
    await user.click(screen.getByTestId("general-repos-edit-confirm"));
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit.mock.calls[0]?.[0]).toEqual([{ ...ROW, url: "https://github.com/acme/renamed" }]);
  });

  it("cancels an edit without committing", async () => {
    const { onCommit } = setup([ROW]);
    const user = userEvent.setup();
    await user.click(screen.getByTestId("general-repos-edit"));
    fireEvent.change(screen.getByTestId("general-repos-edit-url"), { target: { value: "https://github.com/acme/x" } });
    await user.click(screen.getByText("Cancel"));
    expect(onCommit).not.toHaveBeenCalled();
    expect(screen.getByTestId("general-repos-row-url")).toHaveTextContent(ROW.url);
  });

  it("removes a row by committing the list without it", async () => {
    const other: RepoRow = { ...ROW, id: "other-id", url: "https://github.com/acme/other" };
    const { onCommit } = setup([ROW, other]);
    const user = userEvent.setup();
    const rows = screen.getAllByTestId("general-repos-row");
    await user.click(within(rows[0] as HTMLElement).getByTestId("general-repos-remove"));
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit.mock.calls[0]?.[0]).toEqual([other]);
  });

  it("keeps the form open when the write is refused", async () => {
    const user = userEvent.setup();
    const onCommit = vi.fn(async () => false);
    render(
      <RepositoriesSection
        rows={[ROW]}
        disabled={false}
        saving={false}
        serverErrors={{ rows: new Map(), general: [] }}
        onCommit={onCommit}
      />
    );
    await user.click(screen.getByTestId("general-repos-add"));
    fireEvent.change(screen.getByTestId("general-repos-new-url"), { target: { value: "https://github.com/acme/new" } });
    await user.click(screen.getByTestId("general-repos-new-confirm"));
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("general-repos-new-url")).toBeInTheDocument();
  });

  it("shows server refusals on the row and under the section", () => {
    render(
      <RepositoriesSection
        rows={[ROW]}
        disabled={false}
        saving={false}
        serverErrors={{ rows: new Map([[0, "is a duplicate of another repository in the list"]]), general: ["boom"] }}
        onCommit={async () => true}
      />
    );
    expect(screen.getByTestId("general-repos-row-error")).toHaveTextContent(
      "is a duplicate of another repository in the list"
    );
    expect(screen.getByTestId("general-repos-error")).toHaveTextContent("boom");
  });
});
