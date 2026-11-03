// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stacking on a surface with no route project (parity/specs/issues.md
// §14, ISS-167). The workspace all-issues body reads the peek under the
// peeked row's own project while its writer spells that project on every
// write, so the project the mount reads under and the project its writer
// treats as its own are different values. This suite drives the real
// resolving mount through that host shape — only the panel is stubbed —
// because the workspace body's own suite mocks the resolving mount and
// therefore cannot see a stack that re-roots onto its own child.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { peekSearchFor, type PeekRequest, type PeekSelection } from "./peekUrl.js";
import { ResolvingIssuePeek } from "./ResolvingIssuePeek.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ROOT = "323e4567-e89b-12d3-a456-426614174002";
const CHILD = "423e4567-e89b-12d3-a456-426614174003";
const OTHER_ISSUE = "523e4567-e89b-12d3-a456-426614174004";
const OTHER_PROJECT = "623e4567-e89b-12d3-a456-426614174005";

const ROWS = [{ id: ROOT, project_id: PROJECT, sequence_id: 1 }];

// The panel is its own suite's subject; this one owns the stack decision
// the resolving mount makes around it.
vi.mock("./IssuePeek.js", () => ({
  IssuePeek: (props: {
    issueId: string;
    onNestedChange?: ((selection: PeekSelection) => void) | undefined;
    onClose: () => void;
  }) => (
    <section data-testid="peek" data-issue={props.issueId}>
      <button
        type="button"
        onClick={() => props.onNestedChange?.({ issueId: CHILD, projectId: PROJECT, nestingLevel: 1 })}
      >
        Open child
      </button>
      <button
        type="button"
        onClick={() => props.onNestedChange?.({ issueId: props.issueId, projectId: PROJECT, nestingLevel: 0 })}
      >
        Back to parent
      </button>
    </section>
  ),
}));

/** Address writer the workspace routes use: basis "", project always spelled. */
function workspaceWriter(setSearch: React.Dispatch<React.SetStateAction<Record<string, unknown>>>) {
  return (request: PeekRequest): void => {
    setSearch((current) => (request === null ? {} : peekSearchFor(current, request, "")));
  };
}

/**
 * The workspace host: it reads under the peeked project because the route
 * has no project of its own, and it re-renders from the address it wrote.
 */
function WorkspaceHost({ initialSearch }: { initialSearch: Record<string, string> }): React.ReactElement {
  const [search, setSearch] = React.useState<Record<string, unknown>>(initialSearch);
  const peeked = typeof search.peekIssueId === "string" ? search.peekIssueId : null;
  const peekedProject = typeof search.peekProjectId === "string" ? search.peekProjectId : null;
  const held = peeked === null ? undefined : ROWS.find((row) => row.id === peeked);
  const project = peekedProject ?? held?.project_id ?? null;
  if (peeked === null || project === null) return <p>No peek</p>;
  return (
    <>
      <button type="button" onClick={() => setSearch({ peekIssueId: OTHER_ISSUE, peekProjectId: OTHER_PROJECT })}>
        Go elsewhere
      </button>
      <ResolvingIssuePeek
        workspaceSlug={SLUG}
        projectId={project}
        projectIdentifier="WEB"
        addressProjectId=""
        peeked={peeked}
        peekProjectId={peekedProject}
        rows={ROWS}
        onPeekChange={workspaceWriter(setSearch)}
        onClose={() => setSearch({})}
      />
    </>
  );
}

function renderHost(initialSearch: Record<string, string>) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceHost initialSearch={initialSearch} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("ResolvingIssuePeek on a workspace-shaped host", () => {
  it("keeps the parent panel mounted when its nested child takes the address", () => {
    createAppClient("https://api.test", (async () => {
      throw new Error("the panel is stubbed; the mount must not read");
    }) as never);
    renderHost({ peekIssueId: ROOT, peekProjectId: PROJECT });
    expect(screen.getByTestId("peek")).toHaveAttribute("data-issue", ROOT);
    fireEvent.click(screen.getByRole("button", { name: "Open child" }));
    // The host rewrote the address to the child; the stack root stays.
    expect(screen.getByTestId("peek")).toHaveAttribute("data-issue", ROOT);
  });

  it("writes the child id and the child's project when the child is foreign", () => {
    createAppClient("https://api.test", (async () => {
      throw new Error("the panel is stubbed; the mount must not read");
    }) as never);
    const setSearch = vi.fn();
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ResolvingIssuePeek
          workspaceSlug={SLUG}
          projectId={PROJECT}
          projectIdentifier="WEB"
          addressProjectId=""
          peeked={ROOT}
          peekProjectId={PROJECT}
          rows={ROWS}
          onPeekChange={(request) => setSearch(request)}
          onClose={() => {}}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Open child" }));
    expect(setSearch).toHaveBeenCalledWith({ issueId: CHILD, projectId: PROJECT, nestingLevel: 1 });
  });

  it("returns the stack to the parent when the child closes", () => {
    createAppClient("https://api.test", (async () => {
      throw new Error("the panel is stubbed; the mount must not read");
    }) as never);
    renderHost({ peekIssueId: ROOT, peekProjectId: PROJECT });
    fireEvent.click(screen.getByRole("button", { name: "Open child" }));
    fireEvent.click(screen.getByRole("button", { name: "Back to parent" }));
    expect(screen.getByTestId("peek")).toHaveAttribute("data-issue", ROOT);
  });

  it("re-roots when the address names an issue this mount never opened", () => {
    createAppClient("https://api.test", (async () => {
      throw new Error("the panel is stubbed; the mount must not read");
    }) as never);
    renderHost({ peekIssueId: ROOT, peekProjectId: PROJECT });
    fireEvent.click(screen.getByRole("button", { name: "Open child" }));
    // A browser Back (or another list) lands somewhere this mount did not
    // open; the stack drops and the panel follows the address.
    fireEvent.click(screen.getByRole("button", { name: "Go elsewhere" }));
    expect(screen.getByTestId("peek")).toHaveAttribute("data-issue", OTHER_ISSUE);
  });
});
