import { describe, expect, it, vi } from "vitest";
import { EIssueServiceType } from "@pi-dash/types";
import { IssueSubIssuesStore } from "@/store/issue/issue-details/sub_issues.store";
import type { IIssueDetail } from "@/store/issue/issue-details/root.store";

vi.mock("@/services/issue", () => ({
  IssueService: vi.fn(),
}));

vi.mock("@/store/issue/issue-details/sub_issues_filter.store", () => ({
  WorkItemSubIssueFiltersStore: vi.fn(),
}));

const PARENT_ID = "parent-issue-1";
const HELPER_KEY = `${PARENT_ID}_root`;

const makeStore = () => new IssueSubIssuesStore({} as IIssueDetail, EIssueServiceType.ISSUES);

describe("sub-issue helpers store", () => {
  it("keeps toggle semantics on setSubIssueHelpers for the chevron expand/collapse path", () => {
    const store = makeStore();
    store.setSubIssueHelpers(PARENT_ID, "issue_visibility", "child-1");
    expect(store.subIssueHelpersByIssueId(PARENT_ID).issue_visibility).toEqual(["child-1"]);
    store.setSubIssueHelpers(PARENT_ID, "issue_visibility", "child-1");
    expect(store.subIssueHelpersByIssueId(PARENT_ID).issue_visibility).toEqual([]);
  });

  it("markSubIssueHelper is idempotent instead of toggling", () => {
    const store = makeStore();
    store.markSubIssueHelper(HELPER_KEY, "issue_visibility", PARENT_ID);
    store.markSubIssueHelper(HELPER_KEY, "issue_visibility", PARENT_ID);
    expect(store.subIssueHelpersByIssueId(HELPER_KEY).issue_visibility).toEqual([PARENT_ID]);
  });

  it("unmarkSubIssueHelper removes the value and is a no-op when absent", () => {
    const store = makeStore();
    store.markSubIssueHelper(HELPER_KEY, "preview_loader", PARENT_ID);
    store.unmarkSubIssueHelper(HELPER_KEY, "preview_loader", PARENT_ID);
    store.unmarkSubIssueHelper(HELPER_KEY, "preview_loader", PARENT_ID);
    expect(store.subIssueHelpersByIssueId(HELPER_KEY).preview_loader).toEqual([]);
  });

  it("leaves the section visible when the mount effect runs twice (StrictMode)", async () => {
    const store = makeStore();
    let fetchCount = 0;

    // mirrors handleFetchSubIssues in issue-detail-widgets/sub-issues/content.tsx
    const handleFetchSubIssues = async () => {
      const helpers = store.subIssueHelpersByIssueId(HELPER_KEY);
      if (helpers.issue_visibility.includes(PARENT_ID) || helpers.preview_loader.includes(PARENT_ID)) return;
      try {
        store.markSubIssueHelper(HELPER_KEY, "preview_loader", PARENT_ID);
        fetchCount += 1;
        await Promise.resolve();
        store.markSubIssueHelper(HELPER_KEY, "issue_visibility", PARENT_ID);
      } finally {
        store.unmarkSubIssueHelper(HELPER_KEY, "preview_loader", PARENT_ID);
      }
    };

    // StrictMode double-invokes the mount effect before either fetch resolves
    await Promise.all([handleFetchSubIssues(), handleFetchSubIssues()]);

    expect(fetchCount).toBe(1);
    expect(store.subIssueHelpersByIssueId(HELPER_KEY).issue_visibility).toEqual([PARENT_ID]);
    expect(store.subIssueHelpersByIssueId(HELPER_KEY).preview_loader).toEqual([]);
  });
});
