import { describe, expect, it, vi } from "vitest";
import { EIssueServiceType } from "@pi-dash/types";
import type { TIssue } from "@pi-dash/types";
import { IssueStore } from "@/store/issue/issue-details/issue.store";
import type { IIssueDetail } from "@/store/issue/issue-details/root.store";

vi.mock("@/services/issue", () => ({
  IssueService: vi.fn(),
  IssueArchiveService: vi.fn(),
  WorkspaceDraftService: vi.fn(),
}));

describe("issue execution target hydration", () => {
  it.each(["managed_runner", "cloud_agent", "local_runner", null] as const)(
    "preserves the %s executor and assigned pod from issue detail responses",
    (executor) => {
      const addIssue = vi.fn();
      const root = { rootIssueStore: { issues: { addIssue } } } as unknown as IIssueDetail;
      const store = new IssueStore(root, EIssueServiceType.ISSUES);
      store.addIssueToStore({ id: "issue", agent_executor: executor, assigned_pod_id: "pod" } as TIssue);
      expect(addIssue).toHaveBeenCalledWith([{ id: "issue", agent_executor: executor, assigned_pod_id: "pod" }]);
    }
  );

  it("does not clear execution targets when a lite response omits them", () => {
    const addIssue = vi.fn();
    const root = { rootIssueStore: { issues: { addIssue } } } as unknown as IIssueDetail;
    const store = new IssueStore(root, EIssueServiceType.ISSUES);
    store.addIssueToStore({ id: "issue" } as TIssue);
    expect(addIssue).toHaveBeenCalledWith([{ id: "issue" }]);
  });
});
