/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { observable, runInAction } from "mobx";
import { observer } from "mobx-react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";

// The desktop bundle ships desktop-overlay's copy of the peek header, not
// apps/web's. These drive that copy with the real router, the real next/link
// shim and the real IssuePeekUrlSync, so the expand button is exercised
// against the same store->URL sync it races with in the app.
type TPeekIssue = { workspaceSlug: string; projectId: string; issueId: string };

const mocks = vi.hoisted(() => ({
  store: undefined as unknown as { peekIssue: TPeekIssue | undefined },
  issue: undefined as { project_id: string; sequence_id: number } | undefined,
  projectIdentifier: undefined as string | undefined,
  copy: vi.fn(),
}));

vi.mock("@pi-dash/constants", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pi-dash/constants")>()),
  WEB_BASE_URL: "https://pidash.example.com",
}));
vi.mock("@pi-dash/utils", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pi-dash/utils")>()),
  copyTextToClipboard: mocks.copy,
}));
vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { SUCCESS: "success", ERROR: "error" },
  setToast: vi.fn(),
}));
vi.mock("@pi-dash/propel/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@pi-dash/propel/icon-button", () => ({
  IconButton: ({ onClick }: { onClick: React.MouseEventHandler<HTMLButtonElement> }) => (
    <button type="button" aria-label="Copy link" onClick={onClick} />
  ),
}));
vi.mock("@pi-dash/ui", () => ({ CustomSelect: Object.assign(() => null, { Option: () => null }) }));
vi.mock("@pi-dash/i18n", () => ({ useTranslation: () => ({ t: (message: string) => message }) }));
vi.mock("lucide-react", async (importOriginal) => ({
  ...(await importOriginal<typeof import("lucide-react")>()),
  MoveDiagonal: () => <svg data-testid="expand" />,
}));
vi.mock("@/hooks/store/user", () => ({ useUser: () => ({ data: { id: "user" } }) }));
vi.mock("@/hooks/store/use-issue-detail", () => ({
  useIssueDetail: () => ({
    get peekIssue() {
      return mocks.store.peekIssue;
    },
    setPeekIssue: (peekIssue: TPeekIssue | undefined) =>
      runInAction(() => {
        mocks.store.peekIssue = peekIssue;
      }),
    isAnyModalOpen: false,
    issue: { getIssueById: () => mocks.issue },
    removeIssue: vi.fn(),
    archiveIssue: vi.fn(),
    getIsIssuePeeked: () => true,
  }),
}));
vi.mock("@/hooks/store/use-issues", () => ({ useIssues: () => ({ issues: { removeIssue: vi.fn() } }) }));
vi.mock("@/hooks/store/use-project", () => ({
  useProject: () => ({ getProjectIdentifierById: () => mocks.projectIdentifier }),
}));
vi.mock("@/hooks/use-platform-os", () => ({ usePlatformOS: () => ({ isMobile: false }) }));
vi.mock("@/hooks/use-issue-layout-store", () => ({ useIssueStoreType: () => "PROJECT" }));
vi.mock("../../core/components/issues/issue-detail/subscription", () => ({ IssueSubscription: () => null }));
vi.mock("../../core/components/issues/issue-layouts/quick-action-dropdowns", () => ({
  WorkItemDetailQuickActions: () => null,
}));
vi.mock("../../core/components/issues/issue-update-status", () => ({ NameDescriptionUpdateStatus: () => null }));

import { IssuePeekOverviewHeader } from "../../core/components/issues/peek-overview/header";
import { IssuePeekUrlSync } from "../../core/components/issues/peek-overview/url-sync";

const LIST_PATH = "/acme/projects/project-id/issues/";

const Header = (props: { isArchived?: boolean }) => (
  <IssuePeekOverviewHeader
    peekMode="side-peek"
    setPeekMode={vi.fn()}
    removeRoutePeekId={() =>
      runInAction(() => {
        mocks.store.peekIssue = undefined;
      })
    }
    workspaceSlug="acme"
    projectId="project-id"
    issueId="issue-id"
    isArchived={props.isArchived ?? false}
    disabled={false}
    embedIssue={false}
    toggleDeleteIssueModal={vi.fn()}
    toggleArchiveIssueModal={vi.fn()}
    toggleDuplicateIssueModal={vi.fn()}
    toggleEditIssueModal={vi.fn()}
    toggleMoveIssueModal={vi.fn()}
    handleRestoreIssue={vi.fn()}
    isSubmitting="saved"
  />
);

// Same shape as a project work-item list: the URL sync is mounted by the
// layout root, and the peek (hence its header) only while a work item is peeked.
const WorkItemList = observer(function WorkItemList() {
  return (
    <>
      <IssuePeekUrlSync />
      {mocks.store.peekIssue && <Header />}
    </>
  );
});

// The app's routes are code-split, so a navigation to another page stays in
// flight while its module loads. That window is what a second navigation can
// interrupt.
const lazyPage = (text: string) => async () => {
  await new Promise((resolve) => setTimeout(resolve, 5));
  return { Component: () => <div>{text}</div> };
};

const renderApp = (element: React.ReactNode = <WorkItemList />) => {
  const router = createMemoryRouter(
    [
      { path: "/:workspaceSlug/projects/:projectId/issues/", element },
      { path: "/:workspaceSlug/browse/:workItem/", lazy: lazyPage("work item detail") },
    ],
    { initialEntries: [`${LIST_PATH}?peekIssueId=issue-id`] }
  );
  render(<RouterProvider router={router} />);
  return router;
};

const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });

beforeEach(() => {
  mocks.store = observable({
    peekIssue: { workspaceSlug: "acme", projectId: "project-id", issueId: "issue-id" } as TPeekIssue | undefined,
  });
  mocks.issue = { project_id: "project-id", sequence_id: 7 };
  mocks.projectIdentifier = "DESK";
  mocks.copy.mockReset().mockResolvedValue(undefined);
});

describe("desktop peek header: open in full screen", () => {
  it("lands on the work item detail page and stays there", async () => {
    const router = renderApp();
    const expand = (await screen.findByTestId("expand")).closest("a");
    expect(expand?.getAttribute("href")).toBe("/acme/browse/DESK-7/");

    fireEvent.click(expand!);
    await settle();

    expect(router.state.location.pathname).toBe("/acme/browse/DESK-7/");
    expect(screen.getByText("work item detail")).toBeTruthy();
  });

  it("leaves the list entry without the peek param, so Back does not reopen the peek", async () => {
    const router = renderApp();
    fireEvent.click((await screen.findByTestId("expand")).closest("a")!);
    await settle();
    expect(router.state.location.pathname).toBe("/acme/browse/DESK-7/");

    await act(async () => {
      await router.navigate(-1);
    });
    await waitFor(() => expect(router.state.location.pathname).toBe(LIST_PATH));
    expect(router.state.location.search).toBe("");
  });

  it("does not offer a link to /browse/undefined-7/ while the project is not in the store", async () => {
    mocks.projectIdentifier = undefined;
    const router = renderApp(<Header />);

    const icon = await screen.findByTestId("expand");
    expect(icon.closest("a")).toBeNull();
    expect(icon.closest("[aria-disabled]")).not.toBeNull();

    fireEvent.click(icon);
    await settle();
    expect(router.state.location.pathname).toBe(LIST_PATH);
  });

  it("routes an archived work item by the route project id", async () => {
    mocks.issue = undefined;
    mocks.projectIdentifier = undefined;
    renderApp(<Header isArchived />);

    const expand = (await screen.findByTestId("expand")).closest("a");
    expect(expand?.getAttribute("href")).toBe("/acme/projects/project-id/archives/issues/issue-id/");
  });
});

describe("desktop peek header: copy link", () => {
  it("still copies the hosted work item URL, not the bundle origin", async () => {
    renderApp(<Header />);
    fireEvent.click(await screen.findByRole("button", { name: "Copy link" }));
    await settle();
    expect(mocks.copy).toHaveBeenCalledWith("https://pidash.example.com/acme/browse/DESK-7/");
  });
});
