// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  ARCHIVABLE_STATE_GROUPS,
  DELETE_FAILED_BODY,
  DELETE_PERMISSION_TITLE,
  archiveBody,
  archiveHeading,
  deleteBody,
  deleteHeading,
  deleteSuccessBody,
  failureDetail,
  filterMoveTargets,
  isArchivable,
  isDeletePermissionError,
  kindNoun,
  kindTitle,
  mayDeleteWorkItem,
  moveTargets,
  movedWorkItemPath,
  pendingLabel,
  workItemKey,
} from "./rules.js";

describe("nouns and keys", () => {
  it("names each record kind", () => {
    expect(kindNoun("issue")).toBe("work item");
    expect(kindNoun("sub_issue")).toBe("sub-work item");
    expect(kindNoun("epic")).toBe("epic");
    expect(kindTitle("epic")).toBe("Epic");
    expect(kindTitle("sub_issue")).toBe("Sub-work item");
  });

  it("builds the short key from the project code", () => {
    expect(workItemKey("WEB", 42)).toBe("WEB-42");
    expect(workItemKey(undefined, 7)).toBe("-7");
    expect(workItemKey(null, 7)).toBe("-7");
  });
});

describe("archive copy", () => {
  it("heads with the key and says a restore is possible", () => {
    expect(archiveHeading("issue", "WEB", 42)).toBe("Archive work item WEB 42");
    expect(archiveHeading("epic", "WEB", 3)).toBe("Archive epic WEB 3");
    expect(archiveHeading("sub_issue", undefined, 3)).toBe("Archive sub-work item  3");
    expect(archiveBody("issue")).toContain("restored later");
    expect(archiveBody("sub_issue")).toContain("sub-work items");
    expect(archiveBody("epic")).toContain("epics");
  });

  it("labels the pending button per verb and state", () => {
    expect(pendingLabel("archive", false)).toBe("Archive");
    expect(pendingLabel("archive", true)).toBe("Archiving…");
    expect(pendingLabel("delete", false)).toBe("Delete");
    expect(pendingLabel("delete", true)).toBe("Deleting…");
  });

  it("gates archiving on a finished state group", () => {
    expect(ARCHIVABLE_STATE_GROUPS).toEqual(["completed", "cancelled"]);
    expect(isArchivable("completed")).toBe(true);
    expect(isArchivable("cancelled")).toBe(true);
    expect(isArchivable("started")).toBe(false);
    expect(isArchivable(null)).toBe(false);
    expect(isArchivable(undefined)).toBe(false);
  });
});

describe("delete copy and standing", () => {
  it("warns the removal is permanent and names the kind", () => {
    expect(deleteHeading("issue")).toBe("Delete work item");
    expect(deleteHeading("sub_issue")).toBe("Delete sub-work item");
    expect(deleteHeading("epic")).toBe("Delete epic");
    expect(deleteBody("issue", "WEB-9")).toContain("WEB-9");
    expect(deleteBody("epic", "WEB-9")).toContain("permanently removed");
    expect(deleteSuccessBody("sub_issue")).toBe("sub-work item deleted successfully");
    expect(DELETE_FAILED_BODY).toBe("Failed to delete work item");
    expect(DELETE_PERMISSION_TITLE).toBe("You don't have permission to perform this action.");
  });

  it("lets the creator delete regardless of role", () => {
    expect(mayDeleteWorkItem({ viewerId: "u1", creatorId: "u1", projectRole: 15 })).toBe(true);
    expect(mayDeleteWorkItem({ viewerId: "u1", creatorId: "u1", projectRole: null })).toBe(true);
  });

  it("lets a project admin delete someone else's record", () => {
    expect(mayDeleteWorkItem({ viewerId: "u2", creatorId: "u1", projectRole: 20 })).toBe(true);
    expect(mayDeleteWorkItem({ viewerId: "u2", creatorId: null, projectRole: 20 })).toBe(true);
  });

  it("refuses members, guests and unknown viewers", () => {
    expect(mayDeleteWorkItem({ viewerId: "u2", creatorId: "u1", projectRole: 15 })).toBe(false);
    expect(mayDeleteWorkItem({ viewerId: "u2", creatorId: "u1", projectRole: 5 })).toBe(false);
    expect(mayDeleteWorkItem({ viewerId: null, creatorId: null, projectRole: 20 })).toBe(false);
    expect(mayDeleteWorkItem({ viewerId: "", creatorId: "", projectRole: 20 })).toBe(false);
    // A missing creator id never matches the viewer by accident.
    expect(mayDeleteWorkItem({ viewerId: "u1", creatorId: "", projectRole: 15 })).toBe(false);
    expect(mayDeleteWorkItem({ viewerId: "u1", creatorId: undefined, projectRole: undefined })).toBe(false);
  });

  it("recognises the server's permission refusal", () => {
    expect(isDeletePermissionError({ status: 403 })).toBe(true);
    expect(isDeletePermissionError({ message: "Only admin or creator can delete the work item" })).toBe(true);
    expect(isDeletePermissionError({ status: 500, message: "boom" })).toBe(false);
    expect(isDeletePermissionError(new Error("Only admin or creator can delete the work item"))).toBe(true);
    expect(isDeletePermissionError(null)).toBe(false);
    expect(isDeletePermissionError("403")).toBe(false);
  });

  it("prefers the server's detail, then its message, then the fallback", () => {
    expect(failureDetail({ detail: "  denied  " }, "fallback")).toBe("denied");
    expect(failureDetail({ detail: "   " }, "fallback")).toBe("fallback");
    expect(failureDetail({ message: "nope" }, "fallback")).toBe("nope");
    expect(failureDetail(new Error("native"), "fallback")).toBe("native");
    expect(failureDetail(null, "fallback")).toBe("fallback");
    expect(failureDetail("text", "fallback")).toBe("fallback");
  });
});

describe("move targets", () => {
  const projects = [
    { id: "p1", name: "Parity Project", identifier: "PAR" },
    { id: "p2", name: "Second Home", identifier: "SEC" },
    { id: "p3", name: "Hidden", identifier: "" },
  ];

  it("excludes the current project and unresolved rows", () => {
    expect(moveTargets(projects, "p1").map((row) => row.id)).toEqual(["p2"]);
  });

  it("filters on name and identifier, case-insensitively", () => {
    const targets = moveTargets(projects, "p1");
    expect(filterMoveTargets(targets, "second").map((row) => row.id)).toEqual(["p2"]);
    expect(filterMoveTargets(targets, "sec").map((row) => row.id)).toEqual(["p2"]);
    expect(filterMoveTargets(targets, "  ")).toHaveLength(1);
    expect(filterMoveTargets(targets, "zzz")).toEqual([]);
  });

  it("builds the address of the moved record", () => {
    expect(movedWorkItemPath("acme", "SEC", 9)).toBe("/acme/browse/SEC-9/");
  });
});
