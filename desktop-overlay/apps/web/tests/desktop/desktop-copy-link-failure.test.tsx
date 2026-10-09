/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TIssue } from "@pi-dash/types";

// desktopWebUrl() throws when the bundle was baked without a public web
// origin. A copy-link handler that lets that escape leaves the user with an
// unchanged clipboard and no toast, so these drive the real handlers with the
// real desktopWebUrl and only the origin swapped out.
const mocks = vi.hoisted(() => ({ webBase: "", copy: vi.fn(), toast: vi.fn() }));

vi.mock("@pi-dash/constants", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pi-dash/constants")>()),
  get WEB_BASE_URL() {
    return mocks.webBase;
  },
}));
vi.mock("@pi-dash/utils", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pi-dash/utils")>()),
  copyTextToClipboard: mocks.copy,
}));
vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { SUCCESS: "success", ERROR: "error" },
  setToast: mocks.toast,
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
vi.mock("mobx-react", () => ({ observer: (component: unknown) => component }));
vi.mock("next/link", () => ({
  default: ({ href, children }: { href: string; children: React.ReactNode }) => <a href={href}>{children}</a>,
}));
vi.mock("@/hooks/store/user", () => ({ useUser: () => ({ data: { id: "user" } }) }));
vi.mock("@/hooks/store/use-issue-detail", () => ({
  useIssueDetail: () => ({
    issue: { getIssueById: () => ({ id: "issue-id", project_id: "project-id", sequence_id: 7 }) },
    setPeekIssue: vi.fn(),
    removeIssue: vi.fn(),
    archiveIssue: vi.fn(),
    getIsIssuePeeked: () => false,
  }),
}));
vi.mock("@/hooks/store/use-issues", () => ({ useIssues: () => ({ issues: { removeIssue: vi.fn() } }) }));
vi.mock("@/hooks/store/use-project", () => ({ useProject: () => ({ getProjectIdentifierById: () => "DESK" }) }));
vi.mock("@/hooks/use-platform-os", () => ({ usePlatformOS: () => ({ isMobile: false }) }));
vi.mock("@/pi-dash-web/components/issues/issue-layouts/quick-action-dropdowns", () => ({
  createCopyMenuWithDuplication: vi.fn(),
}));
vi.mock("../../core/components/issues/issue-detail/subscription", () => ({ IssueSubscription: () => null }));
vi.mock("../../core/components/issues/issue-layouts/quick-action-dropdowns", () => ({
  WorkItemDetailQuickActions: () => null,
}));
vi.mock("../../core/components/issues/issue-update-status", () => ({ NameDescriptionUpdateStatus: () => null }));

import { useIssueActionHandlers } from "../../core/components/issues/issue-layouts/quick-action-dropdowns/helper";
import { IssuePeekOverviewHeader } from "../../core/components/issues/peek-overview/header";

const WORK_ITEM_URL = "https://pidash.example.com/acme/browse/DESK-7/";

beforeEach(() => {
  mocks.webBase = "";
  mocks.copy.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  mocks.copy.mockReset();
  mocks.toast.mockReset();
});

const expectOnlyAnErrorToast = () => {
  expect(mocks.copy).not.toHaveBeenCalled();
  expect(mocks.toast).toHaveBeenCalledTimes(1);
  expect(mocks.toast).toHaveBeenCalledWith(expect.objectContaining({ type: "error" }));
};

const expectCopiedWithSuccessToast = () => {
  expect(mocks.copy).toHaveBeenCalledWith(WORK_ITEM_URL);
  expect(mocks.toast).toHaveBeenCalledTimes(1);
  expect(mocks.toast).toHaveBeenCalledWith(expect.objectContaining({ type: "success" }));
};

describe("work item quick-action Copy link", () => {
  const renderHandlers = () =>
    renderHook(() =>
      useIssueActionHandlers({
        issue: { id: "issue-id", project_id: "project-id", sequence_id: 7 } as TIssue,
        workspaceSlug: "acme",
        projectIdentifier: "DESK",
        isEditingAllowed: true,
        isDeletingAllowed: true,
        setIssueToEdit: vi.fn(),
        setCreateUpdateIssueModal: vi.fn(),
        setDeleteIssueModal: vi.fn(),
      })
    ).result.current;

  it("shows an error toast when the bundle has no web origin", async () => {
    const { handleCopyIssueLink } = renderHandlers();
    await act(async () => {
      await handleCopyIssueLink();
    });
    expectOnlyAnErrorToast();
  });

  it("copies the hosted link when the web origin is baked in", async () => {
    mocks.webBase = "https://pidash.example.com";
    const { handleCopyIssueLink } = renderHandlers();
    await act(async () => {
      await handleCopyIssueLink();
    });
    expectCopiedWithSuccessToast();
  });
});

describe("peek overview Copy link", () => {
  const clickCopyLink = async () => {
    render(
      <IssuePeekOverviewHeader
        peekMode="side-peek"
        setPeekMode={vi.fn()}
        removeRoutePeekId={vi.fn()}
        workspaceSlug="acme"
        projectId="project-id"
        issueId="issue-id"
        isArchived={false}
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
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    });
  };

  it("shows an error toast when the bundle has no web origin", async () => {
    await clickCopyLink();
    expectOnlyAnErrorToast();
  });

  it("copies the hosted link when the web origin is baked in", async () => {
    mocks.webBase = "https://pidash.example.com";
    await clickCopyLink();
    expectCopiedWithSuccessToast();
  });
});
