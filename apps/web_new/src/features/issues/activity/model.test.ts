// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Feed rules (ISS-194–196): merge order, category mapping, the
// last-toggle guard, persisted choices and each field's sentence.
import type { IssueActivity, IssueComment } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  CLOUD_FEED_FILTERS,
  DEFAULT_FILTERS,
  FEED_FILTERS,
  FEED_FILTER_LABELS,
  WORKLOG_FILTER,
  activityActorName,
  activityFilterOf,
  activitySentence,
  buildFeed,
  composeFeed,
  feedEntryVisible,
  filterFeed,
  flipSort,
  isFilterNarrowed,
  parseStoredFilters,
  parseStoredSort,
  serializeFilters,
  serializeSort,
  sortFeed,
  toggleFeedFilter,
} from "./model.js";

const ADA = "523e4567-e89b-12d3-a456-426614174004";
const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const ISSUE = "223e4567-e89b-12d3-a456-426614174001";

function activity(overrides: Partial<IssueActivity> = {}): IssueActivity {
  return {
    id: "a1",
    issue: ISSUE,
    project: PROJECT,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    actor: ADA,
    actor_detail: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    },
    verb: "created",
    field: null,
    old_value: null,
    new_value: null,
    comment: "",
    attachments: [],
    issue_comment: null,
    old_identifier: null,
    new_identifier: null,
    epoch: null,
    source_data: null,
    issue_detail: null,
    project_detail: null,
    workspace_detail: null,
    created_at: "2026-10-16T10:00:00Z",
    updated_at: "2026-10-16T10:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

function comment(overrides: Partial<IssueComment> = {}): IssueComment {
  return {
    id: "c1",
    issue: ISSUE,
    project: PROJECT,
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    actor: ADA,
    actor_detail: null,
    comment_html: "<p>Hello</p>",
    comment_stripped: "Hello",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "INTERNAL",
    edited_at: null,
    comment_reactions: [],
    speaker_type: "human",
    speaker_label: "",
    speaker_agent_run_id: null,
    parent: null,
    external_source: null,
    external_id: null,
    is_synced: false,
    description: null,
    created_at: "2026-10-16T11:00:00Z",
    updated_at: "2026-10-16T11:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

describe("activity categories", () => {
  it("maps the state and assignee fields to their own toggles", () => {
    expect(activityFilterOf({ field: "state" })).toBe("STATE");
    expect(activityFilterOf({ field: "assignees" })).toBe("ASSIGNEE");
  });

  it("sends only creation rows to the always-visible category", () => {
    expect(activityFilterOf({ field: null })).toBe("DEFAULT");
  });

  it("puts every other field under updates, including the deletion row", () => {
    expect(activityFilterOf({ field: "name" })).toBe("ACTIVITY");
    expect(activityFilterOf({ field: "agent_wait" })).toBe("ACTIVITY");
    expect(activityFilterOf({ field: "issue" })).toBe("ACTIVITY");
  });
});

describe("feed composition", () => {
  const early = activity({ id: "a1" });
  const late = activity({ id: "a2", field: "state", new_value: "Done", created_at: "2026-10-16T12:00:00Z" });
  const note = comment();

  it("merges both sources oldest first by default order", () => {
    const entries = composeFeed([early, late], [note], DEFAULT_FILTERS, "asc");
    expect(entries.map((entry) => entry.id)).toEqual(["a1", "c1", "a2"]);
    expect(entries[1]?.kind).toBe("comment");
  });

  it("reverses for the descending order", () => {
    const entries = composeFeed([early, late], [note], DEFAULT_FILTERS, "desc");
    expect(entries.map((entry) => entry.id)).toEqual(["a2", "c1", "a1"]);
  });

  it("keeps source order for equal timestamps", () => {
    const same = activity({ id: "a3", created_at: early.created_at });
    expect(sortFeed(buildFeed([early, same], []), "asc").map((entry) => entry.id)).toEqual(["a1", "a3"]);
    expect(sortFeed(buildFeed([early, same], []), "desc").map((entry) => entry.id)).toEqual(["a1", "a3"]);
  });

  it("treats an unparseable timestamp as the epoch", () => {
    const broken = activity({ id: "bad", created_at: "not a date" });
    expect(sortFeed(buildFeed([early, broken], []), "asc").map((entry) => entry.id)).toEqual(["bad", "a1"]);
  });

  it("hides a category that is switched off but keeps creation", () => {
    const entries = filterFeed(buildFeed([early, late], [note]), ["COMMENT"]);
    expect(entries.map((entry) => entry.id)).toEqual(["a1", "c1"]);
  });

  it("hides the deletion row with Updates off and shows it with Updates on", () => {
    const deleted = activity({
      id: "a4",
      field: "issue",
      verb: "deleted",
      created_at: "2026-10-16T13:00:00Z",
    });
    expect(filterFeed(buildFeed([early, deleted], []), ["ACTIVITY"]).map((entry) => entry.id)).toEqual(["a1", "a4"]);
    expect(filterFeed(buildFeed([early, deleted], []), ["COMMENT"]).map((entry) => entry.id)).toEqual(["a1"]);
  });

  it("answers visibility per entry kind", () => {
    const built = composeFeed([early, late], [note], DEFAULT_FILTERS, "asc");
    // Creation rows always show, the comment hides, the state row shows.
    expect(built.map((entry) => feedEntryVisible(entry, ["STATE"]))).toEqual([true, false, true]);
  });
});

describe("filter toggles", () => {
  it("turns a filter off and back on in menu order", () => {
    const off = toggleFeedFilter(DEFAULT_FILTERS, "STATE");
    expect(off).toEqual(["ACTIVITY", "COMMENT", "ASSIGNEE"]);
    expect(toggleFeedFilter(["ASSIGNEE", "ACTIVITY"], "COMMENT")).toEqual(["ACTIVITY", "COMMENT", "ASSIGNEE"]);
  });

  it("refuses to remove the last filter", () => {
    expect(toggleFeedFilter(["COMMENT"], "COMMENT")).toEqual(["COMMENT"]);
    expect(toggleFeedFilter(["ACTIVITY"], "ACTIVITY")).toEqual(["ACTIVITY"]);
  });

  it("marks the control once anything is off", () => {
    expect(isFilterNarrowed(DEFAULT_FILTERS)).toBe(false);
    expect(isFilterNarrowed(["ACTIVITY", "COMMENT", "STATE"])).toBe(true);
  });

  it("flips the order both ways", () => {
    expect(flipSort("asc")).toBe("desc");
    expect(flipSort("desc")).toBe("asc");
  });
});

describe("stored choices", () => {
  it("round-trips the toggles", () => {
    const stored = serializeFilters(["STATE", "ACTIVITY"]);
    expect(stored).toBe('["ACTIVITY","STATE"]');
    expect(parseStoredFilters(stored)).toEqual(["ACTIVITY", "STATE"]);
  });

  it("round-trips the order", () => {
    expect(serializeSort("desc")).toBe('"desc"');
    expect(parseStoredSort('"desc"')).toBe("desc");
    expect(parseStoredSort(null)).toBe("asc");
    expect(parseStoredSort('"asc"')).toBe("asc");
  });

  it("falls back to all four for missing, junk or unknown values", () => {
    expect(parseStoredFilters(null)).toEqual([...FEED_FILTERS]);
    expect(parseStoredFilters("not json")).toEqual([...FEED_FILTERS]);
    expect(parseStoredFilters('"COMMENT"')).toEqual([...FEED_FILTERS]);
    expect(parseStoredFilters("[]")).toEqual([...FEED_FILTERS]);
    expect(parseStoredFilters('["NOPE","COMMENT"]')).toEqual(["COMMENT"]);
    expect(parseStoredSort("not json")).toBe("asc");
    expect(parseStoredSort('"whatever"')).toBe("asc");
  });
});

describe("activity sentences", () => {
  it("describes creation and deletion", () => {
    expect(activitySentence({ field: null, verb: "created", old_value: null, new_value: null })).toBe(
      "created the work item."
    );
    expect(activitySentence({ field: null, verb: "deleted", old_value: null, new_value: null })).toBe(
      "deleted the work item."
    );
    expect(activitySentence({ field: "issue", verb: "deleted", old_value: null, new_value: null })).toBe(
      "deleted the work item."
    );
  });

  it("describes the scalar properties", () => {
    expect(activitySentence({ field: "name", verb: "updated", old_value: "Old", new_value: "Renamed issue" })).toBe(
      "renamed the work item to “Renamed issue”."
    );
    expect(activitySentence({ field: "description", verb: "updated", old_value: null, new_value: null })).toBe(
      "edited the description."
    );
    expect(activitySentence({ field: "state", verb: "updated", old_value: "Todo", new_value: "Done" })).toBe(
      "moved the state to Done."
    );
    expect(activitySentence({ field: "priority", verb: "updated", old_value: null, new_value: "high" })).toBe(
      "set the priority to high."
    );
    expect(activitySentence({ field: "estimate_point", verb: "updated", old_value: null, new_value: "3" })).toBe(
      "set the estimate to 3."
    );
    expect(activitySentence({ field: "estimate_points", verb: "updated", old_value: "3", new_value: "" })).toBe(
      "cleared the estimate."
    );
    expect(activitySentence({ field: "estimate_categories", verb: "updated", old_value: "3", new_value: "1" })).toBe(
      "set the estimate to 1."
    );
    expect(activitySentence({ field: "start_date", verb: "updated", old_value: "2026-09-01", new_value: "" })).toBe(
      "cleared the start date."
    );
    expect(activitySentence({ field: "start_date", verb: "updated", old_value: null, new_value: "2026-10-01" })).toBe(
      "set the start date to 2026-10-01."
    );
    expect(activitySentence({ field: "target_date", verb: "updated", old_value: "2026-10-09", new_value: "" })).toBe(
      "cleared the due date."
    );
    expect(activitySentence({ field: "target_date", verb: "updated", old_value: null, new_value: "2026-10-09" })).toBe(
      "set the due date to 2026-10-09."
    );
  });

  it("describes assignees and labels by add or remove", () => {
    expect(activitySentence({ field: "assignees", verb: "created", old_value: "", new_value: "Bob" })).toBe(
      "assigned the work item to Bob."
    );
    expect(activitySentence({ field: "assignees", verb: "deleted", old_value: "Bob", new_value: "" })).toBe(
      "removed Bob from the assignees."
    );
    expect(activitySentence({ field: "labels", verb: "created", old_value: "", new_value: "bug" })).toBe(
      "added the label bug."
    );
    expect(activitySentence({ field: "labels", verb: "deleted", old_value: "bug", new_value: "" })).toBe(
      "removed the label bug."
    );
  });

  it("describes parents", () => {
    expect(activitySentence({ field: "parent", verb: "updated", old_value: "", new_value: "PAR-9" })).toBe(
      "set the parent to PAR-9."
    );
    expect(activitySentence({ field: "parent", verb: "updated", old_value: "PAR-9", new_value: "" })).toBe(
      "removed the parent PAR-9."
    );
  });

  it("describes each relation kind both ways", () => {
    expect(activitySentence({ field: "blocking", verb: "created", old_value: "", new_value: "PAR-1" })).toBe(
      "marked the work item as blocking PAR-1."
    );
    expect(activitySentence({ field: "blocking", verb: "deleted", old_value: "PAR-1", new_value: "" })).toBe(
      "dropped the blocking link to PAR-1."
    );
    expect(activitySentence({ field: "blocked_by", verb: "created", old_value: "", new_value: "PAR-2" })).toBe(
      "marked the work item as blocked by PAR-2."
    );
    expect(activitySentence({ field: "blocked_by", verb: "deleted", old_value: "PAR-2", new_value: "" })).toBe(
      "dropped the blocked-by link to PAR-2."
    );
    expect(activitySentence({ field: "duplicate", verb: "created", old_value: "", new_value: "PAR-3" })).toBe(
      "marked the work item as a duplicate of PAR-3."
    );
    expect(activitySentence({ field: "duplicate", verb: "deleted", old_value: "PAR-3", new_value: "" })).toBe(
      "dropped the duplicate link to PAR-3."
    );
    expect(activitySentence({ field: "relates_to", verb: "created", old_value: "", new_value: "PAR-4" })).toBe(
      "linked the work item to PAR-4."
    );
    expect(activitySentence({ field: "relates_to", verb: "deleted", old_value: "PAR-4", new_value: "" })).toBe(
      "unlinked the work item from PAR-4."
    );
  });

  it("describes cycles and modules for every verb", () => {
    expect(activitySentence({ field: "cycles", verb: "created", old_value: "", new_value: "Sprint 4" })).toBe(
      "added the work item to the cycle Sprint 4."
    );
    expect(activitySentence({ field: "cycles", verb: "updated", old_value: "Sprint 3", new_value: "Sprint 4" })).toBe(
      "moved the work item to the cycle Sprint 4."
    );
    expect(activitySentence({ field: "cycles", verb: "deleted", old_value: "Sprint 4", new_value: "" })).toBe(
      "removed the work item from the cycle Sprint 4."
    );
    expect(activitySentence({ field: "modules", verb: "created", old_value: "", new_value: "Core" })).toBe(
      "added the work item to the module Core."
    );
    expect(activitySentence({ field: "modules", verb: "updated", old_value: "Edge", new_value: "Core" })).toBe(
      "moved the work item to the module Core."
    );
    expect(activitySentence({ field: "modules", verb: "deleted", old_value: "Core", new_value: "" })).toBe(
      "removed the work item from the module Core."
    );
  });

  it("describes links and attachments", () => {
    expect(activitySentence({ field: "link", verb: "created", old_value: "", new_value: "https://x" })).toBe(
      "attached a link."
    );
    expect(activitySentence({ field: "link", verb: "updated", old_value: "https://x", new_value: "https://y" })).toBe(
      "changed the attached link."
    );
    expect(activitySentence({ field: "link", verb: "deleted", old_value: "https://x", new_value: "" })).toBe(
      "removed the attached link."
    );
    expect(activitySentence({ field: "attachment", verb: "created", old_value: "", new_value: "a.pdf" })).toBe(
      "attached a file."
    );
    expect(activitySentence({ field: "attachment", verb: "deleted", old_value: "a.pdf", new_value: "" })).toBe(
      "removed an attachment."
    );
  });

  it("describes archive, agent wait/wake and intake verbs", () => {
    expect(activitySentence({ field: "archived_at", verb: "updated", old_value: "false", new_value: "archive" })).toBe(
      "archived the work item."
    );
    expect(activitySentence({ field: "archived_at", verb: "updated", old_value: "true", new_value: "restore" })).toBe(
      "restored the work item."
    );
    expect(activitySentence({ field: "agent_wait", verb: "created", old_value: "2", new_value: "PAR-1" })).toBe(
      "started waiting on PAR-1 (2)."
    );
    expect(activitySentence({ field: "agent_wake", verb: "created", old_value: "completed", new_value: "PAR-1" })).toBe(
      "resumed after PAR-1 was completed."
    );
    expect(activitySentence({ field: "intake", verb: "1", old_value: null, new_value: null })).toBe(
      "accepted the intake item."
    );
    expect(activitySentence({ field: "inbox", verb: "-1", old_value: null, new_value: null })).toBe(
      "declined the intake item."
    );
    expect(activitySentence({ field: "inbox", verb: "0", old_value: null, new_value: null })).toBe(
      "snoozed the intake item."
    );
    expect(activitySentence({ field: "inbox", verb: "2", old_value: null, new_value: null })).toBe(
      "declined the intake item as a duplicate."
    );
    expect(activitySentence({ field: "intake", verb: "9", old_value: null, new_value: null })).toBe(
      "changed the intake status."
    );
  });

  it("renders nothing for fields the timeline does not describe", () => {
    expect(activitySentence({ field: "type", verb: "updated", old_value: null, new_value: null })).toBeNull();
    expect(activitySentence({ field: "something_new", verb: "updated", old_value: null, new_value: null })).toBeNull();
  });
});

describe("worklog category", () => {
  it("maps the worklog field to its own toggle", () => {
    expect(activityFilterOf({ field: "worklog" })).toBe(WORKLOG_FILTER);
    expect(WORKLOG_FILTER).toBe("WORKLOG");
    expect(FEED_FILTER_LABELS[WORKLOG_FILTER]).toBe("Worklog");
    expect(CLOUD_FEED_FILTERS).toEqual(["ACTIVITY", "COMMENT", "STATE", "ASSIGNEE", "WORKLOG"]);
  });

  it("shows worklog rows only while their toggle is on", () => {
    const row = activity({ id: "w1", field: "worklog", verb: "created", new_value: "90" });
    const [entry] = buildFeed([row], []);
    expect(entry).toBeDefined();
    if (entry === undefined) return;
    expect(feedEntryVisible(entry, CLOUD_FEED_FILTERS)).toBe(true);
    expect(feedEntryVisible(entry, ["WORKLOG"])).toBe(true);
    expect(feedEntryVisible(entry, FEED_FILTERS)).toBe(false);
  });

  it("toggles the fifth category in menu order and guards the last one", () => {
    expect(toggleFeedFilter(CLOUD_FEED_FILTERS, "WORKLOG", CLOUD_FEED_FILTERS)).toEqual([...FEED_FILTERS]);
    expect(toggleFeedFilter(FEED_FILTERS, "WORKLOG", CLOUD_FEED_FILTERS)).toEqual([...CLOUD_FEED_FILTERS]);
    expect(toggleFeedFilter(["WORKLOG"], "WORKLOG", CLOUD_FEED_FILTERS)).toEqual(["WORKLOG"]);
  });

  it("marks four-of-five as narrowed in cloud but full in OSS", () => {
    expect(isFilterNarrowed(CLOUD_FEED_FILTERS, CLOUD_FEED_FILTERS)).toBe(false);
    expect(isFilterNarrowed([...FEED_FILTERS], CLOUD_FEED_FILTERS)).toBe(true);
    expect(isFilterNarrowed([...FEED_FILTERS])).toBe(false);
  });

  it("defaults cloud to five toggles and drops stored worklog in OSS", () => {
    expect(parseStoredFilters(null, CLOUD_FEED_FILTERS)).toEqual([...CLOUD_FEED_FILTERS]);
    expect(parseStoredFilters("not json", CLOUD_FEED_FILTERS)).toEqual([...CLOUD_FEED_FILTERS]);
    expect(parseStoredFilters('["WORKLOG","STATE"]', CLOUD_FEED_FILTERS)).toEqual(["STATE", "WORKLOG"]);
    expect(parseStoredFilters('["WORKLOG","STATE"]')).toEqual(["STATE"]);
    expect(parseStoredFilters('["WORKLOG"]')).toEqual([...FEED_FILTERS]);
  });

  it("sentences a fresh log from its minutes", () => {
    expect(activitySentence({ field: "worklog", verb: "created", old_value: null, new_value: "90" })).toBe(
      "logged 1h 30m."
    );
    expect(activitySentence({ field: "worklog", verb: "created", old_value: null, new_value: "45" })).toBe(
      "logged 45m."
    );
  });

  it("falls back to a plain sentence for empty or unparseable minutes", () => {
    expect(activitySentence({ field: "worklog", verb: "created", old_value: null, new_value: null })).toBe(
      "logged time."
    );
    expect(activitySentence({ field: "worklog", verb: "created", old_value: null, new_value: "soon" })).toBe(
      "logged time."
    );
    expect(activitySentence({ field: "worklog", verb: "created", old_value: null, new_value: "0" })).toBe(
      "logged time."
    );
  });

  it("sentences an edit from its before and after minutes", () => {
    expect(activitySentence({ field: "worklog", verb: "updated", old_value: "30", new_value: "90" })).toBe(
      "changed logged time from 30m to 1h 30m."
    );
  });

  it("reads an edit with unchanged minutes as a plain edit", () => {
    expect(activitySentence({ field: "worklog", verb: "updated", old_value: "90", new_value: "90" })).toBe(
      "edited a time entry."
    );
  });

  it("falls back to a plain sentence for unparseable edits", () => {
    expect(activitySentence({ field: "worklog", verb: "updated", old_value: "soon", new_value: "90" })).toBe(
      "edited a time entry."
    );
    expect(activitySentence({ field: "worklog", verb: "updated", old_value: "", new_value: "90" })).toBe(
      "edited a time entry."
    );
    expect(activitySentence({ field: "worklog", verb: "updated", old_value: "30", new_value: "soon" })).toBe(
      "edited a time entry."
    );
    expect(activitySentence({ field: "worklog", verb: "updated", old_value: "30", new_value: "0" })).toBe(
      "edited a time entry."
    );
  });

  it("sentences a removal", () => {
    expect(activitySentence({ field: "worklog", verb: "deleted", old_value: "90", new_value: null })).toBe(
      "removed logged time."
    );
  });
});

describe("activity actor", () => {
  it("credits the product for its own actions", () => {
    expect(activityActorName({ field: "agent_wait", new_value: "PAR-1", actor_detail: null })).toBe("Pi Dash");
    expect(activityActorName({ field: "agent_wake", new_value: "PAR-1", actor_detail: null })).toBe("Pi Dash");
    expect(activityActorName({ field: "archived_at", new_value: "archive", actor_detail: null })).toBe("Pi Dash");
  });

  it("names the member otherwise, even on a restore", () => {
    const detail = {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    };
    expect(activityActorName({ field: "state", new_value: "Done", actor_detail: detail })).toBe("Ada L");
    expect(activityActorName({ field: "archived_at", new_value: "restore", actor_detail: detail })).toBe("Ada L");
    expect(activityActorName({ field: "state", new_value: "Done", actor_detail: null })).toBe("Unknown author");
  });
});
