/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pushSpy, setPeekIssue, getIssueById, getProjectIdentifierById, getIsIssuePeeked } = vi.hoisted(() => ({
  pushSpy: vi.fn(),
  setPeekIssue: vi.fn(),
  getIssueById: vi.fn(),
  getProjectIdentifierById: vi.fn(),
  getIsIssuePeeked: vi.fn(),
}));

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@pi-dash/types", () => ({
  EIssuesStoreType: { ARCHIVED: "ARCHIVED", EPIC: "EPIC", PROJECT: "PROJECT" },
}));

// eslint-disable-next-line @typescript-eslint/no-explicit-any
vi.mock("lucide-react", () => ({
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  MoveDiagonal: (props: any) => <svg data-testid="move-diagonal" {...props} />,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  MoveRight: (props: any) => <svg data-testid="move-right" {...props} />,
}));

vi.mock("next/link", () => ({
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  default: ({ href, onClick, children }: any) => (
    <a href={href} onClick={onClick}>
      {children}
    </a>
  ),
}));

vi.mock("@pi-dash/propel/icons", () => ({
  CenterPanelIcon: () => <svg />,
  CopyLinkIcon: () => <svg />,
  FullScreenPanelIcon: () => <svg />,
  SidePanelIcon: () => <svg />,
}));

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { SUCCESS: "success", ERROR: "error" },
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/propel/tooltip", () => ({
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  Tooltip: ({ children }: any) => <>{children}</>,
}));

vi.mock("@pi-dash/propel/icon-button", () => ({
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  IconButton: (props: any) => <button type="button" onClick={props.onClick} />,
}));

vi.mock("@pi-dash/ui", () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any, unicorn/consistent-function-scoping
  const CustomSelect: any = ({ children, customButton }: any) => (
    <div>
      {customButton}
      {children}
    </div>
  );
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  CustomSelect.Option = ({ children }: any) => <div>{children}</div>;
  return { CustomSelect };
});

vi.mock("@pi-dash/utils", () => ({
  copyUrlToClipboard: vi.fn(),
  // Mirror the real generateWorkItemLink branch semantics so assertions match.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  generateWorkItemLink: (args: any) =>
    args.isArchived
      ? `/${args.workspaceSlug}/projects/${args.projectId}/archives/issues/${args.issueId}`
      : `/${args.workspaceSlug}/browse/${args.projectIdentifier}-${args.sequenceId}/`,
}));

vi.mock("@/hooks/use-app-router", () => ({
  useAppRouter: () => ({ push: pushSpy }),
}));

vi.mock("@/hooks/store/use-issue-detail", () => ({
  useIssueDetail: () => ({
    issue: { getIssueById },
    setPeekIssue,
    removeIssue: vi.fn(),
    archiveIssue: vi.fn(),
    getIsIssuePeeked,
  }),
}));

vi.mock("@/hooks/store/use-issues", () => ({
  useIssues: () => ({ issues: { removeIssue: vi.fn() } }),
}));

vi.mock("@/hooks/store/use-project", () => ({
  useProject: () => ({ getProjectIdentifierById }),
}));

vi.mock("@/hooks/store/user", () => ({
  useUser: () => ({ data: null }),
}));

vi.mock("@/hooks/use-platform-os", () => ({
  usePlatformOS: () => ({ isMobile: false }),
}));

vi.mock("@/components/issues/issue-detail/subscription", () => ({
  IssueSubscription: () => null,
}));

vi.mock("@/components/issues/issue-layouts/quick-action-dropdowns", () => ({
  WorkItemDetailQuickActions: () => null,
}));

vi.mock("@/components/issues/issue-update-status", () => ({
  NameDescriptionUpdateStatus: () => null,
}));

import { IssuePeekOverviewHeader } from "../../core/components/issues/peek-overview/header";

function renderHeader(overrides: { isArchived?: boolean } = {}) {
  const removeRoutePeekId = vi.fn();
  render(
    <IssuePeekOverviewHeader
      peekMode="side-peek"
      setPeekMode={vi.fn()}
      removeRoutePeekId={removeRoutePeekId}
      workspaceSlug="acme"
      projectId="proj-1"
      issueId="issue-1"
      isArchived={overrides.isArchived ?? false}
      disabled={false}
      embedIssue={false}
      toggleDeleteIssueModal={vi.fn()}
      toggleArchiveIssueModal={vi.fn()}
      toggleDuplicateIssueModal={vi.fn()}
      toggleEditIssueModal={vi.fn()}
      toggleMoveIssueModal={vi.fn()}
      handleRestoreIssue={vi.fn()}
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      isSubmitting={"saved" as any}
    />
  );
  return { removeRoutePeekId };
}

describe("IssuePeekOverviewHeader expand button", () => {
  beforeEach(() => {
    pushSpy.mockReset();
    setPeekIssue.mockReset();
    getIssueById.mockReset();
    getProjectIdentifierById.mockReset();
    getIsIssuePeeked.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("clears the peek before navigating to the loaded work item detail route", () => {
    getIssueById.mockReturnValue({ project_id: "proj-1", sequence_id: 42 });
    getProjectIdentifierById.mockReturnValue("PROJ");

    const { removeRoutePeekId } = renderHeader();

    const expand = screen.getByTestId("move-diagonal").closest("a");
    expect(expand).not.toBeNull();
    expect(expand?.getAttribute("href")).toBe("/acme/browse/PROJ-42/");

    const notPrevented = fireEvent.click(expand!);
    // default is prevented -> react-router-style navigation is suppressed in favor
    // of the programmatic push.
    expect(notPrevented).toBe(false);

    expect(removeRoutePeekId).toHaveBeenCalledTimes(1);
    expect(pushSpy).toHaveBeenCalledWith("/acme/browse/PROJ-42/");
    // Peek is torn down BEFORE navigation so the store->URL sync strips
    // ?peekIssueId from the list entry before the push lands.
    expect(removeRoutePeekId.mock.invocationCallOrder[0]).toBeLessThan(pushSpy.mock.invocationCallOrder[0]);
  });

  it("routes archived items to the archive detail path", () => {
    getIssueById.mockReturnValue({ project_id: "proj-1", sequence_id: 7 });
    getProjectIdentifierById.mockReturnValue("PROJ");

    renderHeader({ isArchived: true });

    const expand = screen.getByTestId("move-diagonal").closest("a");
    expect(expand?.getAttribute("href")).toBe("/acme/projects/proj-1/archives/issues/issue-1");
  });

  it("keeps the archive link valid while the issue detail is still loading", () => {
    // Detail not yet in the store: issueDetails?.project_id is undefined, but the
    // archive route must use the route projectId prop, not /projects/undefined/...
    getIssueById.mockReturnValue(undefined);
    getProjectIdentifierById.mockReturnValue(undefined);

    renderHeader({ isArchived: true });

    const expand = screen.getByTestId("move-diagonal").closest("a");
    expect(expand?.getAttribute("href")).toBe("/acme/projects/proj-1/archives/issues/issue-1");
  });

  it("lets modified clicks fall through to native navigation without tearing down the peek", () => {
    getIssueById.mockReturnValue({ project_id: "proj-1", sequence_id: 42 });
    getProjectIdentifierById.mockReturnValue("PROJ");

    const { removeRoutePeekId } = renderHeader();

    const expand = screen.getByTestId("move-diagonal").closest("a");
    const notPrevented = fireEvent.click(expand!, { metaKey: true });

    expect(notPrevented).toBe(true);
    expect(removeRoutePeekId).not.toHaveBeenCalled();
    expect(pushSpy).not.toHaveBeenCalled();
  });

  it("disables the expand control while the issue detail is still loading", () => {
    // Not yet in the store -> no projectIdentifier / sequence_id to build PROJ-123.
    getIssueById.mockReturnValue(undefined);
    getProjectIdentifierById.mockReturnValue(undefined);

    const { removeRoutePeekId } = renderHeader();

    const icon = screen.getByTestId("move-diagonal");
    // No anchor -> no navigation to /browse/undefined-undefined/.
    expect(icon.closest("a")).toBeNull();
    const disabledWrap = icon.closest("[aria-disabled]");
    expect(disabledWrap).not.toBeNull();

    fireEvent.click(icon);
    expect(removeRoutePeekId).not.toHaveBeenCalled();
    expect(pushSpy).not.toHaveBeenCalled();
  });
});
