// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-377): analytics custom-insights section. Rows
// ANL-010 (axis pickers stay mutually exclusive, the metric picker offers
// only the in-scope count metric — estimate and the off-scope epic/item
// metric are never offered, even with points estimation on — and every
// re-picker re-queries), ANL-011 (the bar chart stays single-series without
// a second split and stacks with one, priority bars use fixed per-priority
// hues, state bars reuse workspace state colors, date axes group by day
// with year-aware labels, past-ten segments extend the theme palette, bar
// ends round only on true segment edges, and no legend renders — the hover
// tooltip carries the segments), ANL-012 (the table under the chart repeats
// the plotted values with a live first-column search and an export of the
// filtered rows). Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner and workspace, drains
// the backend's demo seed (NEWFRONT-265), and creates exactly the projects
// it needs — scenario project names never reuse the workspace name, or the
// drain cannot tell them apart.
//
// Seeding note: moving an issue into a ticking state (started) makes the
// server try an agent dispatch, and a scratch stack with no runners bounces
// the issue back to Backlog with a system comment. Seeded state moves below
// ride the client's skip-dispatch header so the planted group sticks; the
// resulting server state is the legitimate seeded state either way.
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  analyticsAdvanceCustomChartsRaw,
  analyticsProjectStateColors,
  browserSessionCookies,
  createIssue,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  parityProjectIdentifier,
  projectStates,
  serverCreateEstimate,
  serverCreateLabel,
  serverProjectEstimate,
  serverSetIssueLabels,
  serverSetProjectEstimate,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW_010 = ["ANL-010"];
const ROW_011 = ["ANL-011"];
const ROW_012 = ["ANL-012"];

interface AnalyticsWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  workspaceName: string;
  tag: string;
}

/** Fresh owner plus workspace with the backend seed drained (zero projects). */
async function setupAnalyticsWorld(prefix: string): Promise<AnalyticsWorld> {
  const owner = await signUpAuthedSession(prefix);
  const tag = uniqueSuffixForProjects();
  const workspaceName = `Anl WS ${tag}`;
  const ws = await createWorkspaceForProjects(owner, { name: workspaceName, slug: `anl-${tag}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  await drainWorkspaceSeed(owner, ws.slug, workspaceName);
  return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, workspaceName, tag };
}

/** Create one project; retries past identifier collisions like viewsHarness. */
async function setupAnalyticsProject(
  world: AnalyticsWorld,
  name: string,
  identifierPrefix: string
): Promise<{ id: string; name: string; identifier: string }> {
  let id = "";
  let identifier = "";
  for (let attempt = 0; attempt < 4; attempt += 1) {
    identifier = parityProjectIdentifier(identifierPrefix);
    try {
      id = await createProjectViaApi(world.owner, world.workspaceSlug, { name, identifier });
      break;
    } catch (error) {
      if (attempt === 3 || !(error instanceof Error) || !/HTTP 400/.test(error.message)) throw error;
    }
  }
  return { id, name, identifier };
}

/** Enter an analytics tab authenticated as `session`. */
async function enterAnalyticsTab(
  driver: ParityDriver,
  world: AnalyticsWorld,
  tabId: string,
  session: AuthedSession = world.owner
): Promise<void> {
  await driver.analyticsOpenTab(world.workspaceSlug, tabId, browserSessionCookies(session));
}

/**
 * Move one seeded issue, skipping the agent dispatch the scratch stack could
 * never serve (see the file header). Throttle-tolerant like `authed`.
 */
async function moveSeededIssue(
  world: AnalyticsWorld,
  projectId: string,
  issueId: string,
  stateId: string
): Promise<void> {
  const backoffMs = [3000, 6000, 12000];
  for (let attempt = 0; ; attempt += 1) {
    const res = await fetch(
      `${world.owner.apiBase}/api/workspaces/${world.workspaceSlug}/projects/${projectId}/issues/${issueId}/`,
      {
        method: "PATCH",
        headers: {
          "content-type": "application/json",
          cookie: world.owner.cookie,
          "X-Pi-Dash-Skip-Immediate-Dispatch": "1",
        },
        body: JSON.stringify({ state_id: stateId }),
      }
    );
    if (res.status === 429 && attempt < backoffMs.length) {
      await new Promise((resolve) => setTimeout(resolve, backoffMs[attempt]));
      continue;
    }
    if (!res.ok) throw new Error(`[parity] seeded state move failed with HTTP ${res.status}.`);
    return;
  }
}

/** Custom-graph payload rows (`type=custom-work-items` reads data+schema). */
function customRowsOf(
  payload: unknown
): Array<{ key: string; name: string; count: number; extra: Record<string, number> }> {
  if (typeof payload !== "object" || payload === null) return [];
  const data = (payload as Record<string, unknown>)["data"];
  if (!Array.isArray(data)) return [];
  const rows: Array<{ key: string; name: string; count: number; extra: Record<string, number> }> = [];
  for (const row of data) {
    if (typeof row !== "object" || row === null) continue;
    const record = row as Record<string, unknown>;
    if (typeof record["key"] !== "string" || typeof record["name"] !== "string") continue;
    if (typeof record["count"] !== "number") continue;
    const extra: Record<string, number> = {};
    for (const [field, value] of Object.entries(record)) {
      if (field !== "key" && field !== "name" && field !== "count" && typeof value === "number") extra[field] = value;
    }
    rows.push({ key: record["key"], name: record["name"], count: record["count"], extra });
  }
  return rows;
}

/** Custom-graph schema map (segment key to display label). */
function customSchemaOf(payload: unknown): Record<string, string> {
  if (typeof payload !== "object" || payload === null) return {};
  const schema = (payload as Record<string, unknown>)["schema"];
  if (typeof schema !== "object" || schema === null || Array.isArray(schema)) return {};
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(schema as Record<string, unknown>)) {
    if (typeof value === "string") out[key] = value;
  }
  return out;
}

/** X-axis property labels in picker order, as the old app offers them. */
const X_AXIS_LABELS = [
  "State name",
  "State group",
  "Priority",
  "Label",
  "Assignee",
  "Estimate point",
  "Cycle",
  "Module",
  "Completed date",
  "Due date",
  "Start date",
  "Created date",
];

/** Fixed single-series hues by rendered priority label. */
const PRIORITY_HUES: Record<string, string> = {
  Urgent: "#ef4444",
  High: "#f97316",
  Medium: "#eab308",
  Low: "#22c55e",
  None: "#ced4da",
};

/** First theme palette slots (light), taken by stacked segments in schema order. */
const PALETTE_SLOTS = [
  "#6172e8",
  "#8b6edb",
  "#e05f99",
  "#29a383",
  "#cb8a37",
  "#3aa7c1",
  "#f1b24a",
  "#e84855",
  "#50c799",
  "#b35f9e",
];

test(
  specTitle(ROW_010, "axis pickers exclude each other, metric hides estimate, every re-picker re-queries"),
  { tag: specTags(ROW_010) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl010");
    const project = await setupAnalyticsProject(world, `Anl Pickers ${world.tag.slice(-4)}`, "AP");
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "pick urgent",
      { priority: "urgent" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "pick high",
      { priority: "high" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "pick low",
      { priority: "low" },
      world.owner.apiBase
    );

    await enterAnalyticsTab(driver, world, "work-items");

    await test.step("the pickers open on the metric, priority, and an empty split", async () => {
      expect(await driver.analyticsCustomPickerLabels()).toEqual({
        metric: "Work item",
        xAxis: "Priority",
        split: "Add Property",
      });
    });

    await test.step("the metric picker offers only the in-scope count", async () => {
      // The estimate option and the off-scope epic/item metric never render
      // as options — see the ANL-010 row note.
      expect(await driver.analyticsCustomPickerOptions(0)).toEqual(["Work item"]);
    });

    await test.step("points estimation still offers only the in-scope count", async () => {
      const estimate = await serverCreateEstimate(
        world.workspaceSlug,
        project.id,
        "Anl Points",
        ["1", "2", "3"],
        world.owner.cookie,
        "points",
        world.owner.apiBase
      );
      await serverSetProjectEstimate(
        world.workspaceSlug,
        project.id,
        estimate.id,
        world.owner.cookie,
        world.owner.apiBase
      );
      expect(
        await serverProjectEstimate(world.workspaceSlug, project.id, world.owner.cookie, world.owner.apiBase)
      ).toBe(estimate.id);
      await enterAnalyticsTab(driver, world, "work-items");
      expect(await driver.analyticsCustomPickerOptions(0)).toEqual(["Work item"]);
    });

    await test.step("the x picker offers every dimension while the split excludes the chosen one", async () => {
      expect(await driver.analyticsCustomPickerOptions(1)).toEqual(X_AXIS_LABELS);
      expect(await driver.analyticsCustomPickerOptions(2)).toEqual([
        "No value",
        ...X_AXIS_LABELS.filter((label) => label !== "Priority"),
      ]);
    });

    await test.step("choosing a split re-queries and narrows the x picker", async () => {
      await driver.analyticsStartRequestSpy();
      await driver.analyticsCustomPickerSelect(2, "State name");
      // The new client addresses the endpoint with a trailing slash;
      // the oracle without. The match ignores it: the endpoint and
      // its parameters are the assertion, not the slash.
      await expect
        .poll(
          async () =>
            (await driver.analyticsSpyRequests()).filter((request) => {
              const query = new URLSearchParams(request.query);
              return (
                request.path.replace(/\/$/, "").endsWith("/advance-analytics-charts") &&
                query.get("type") === "custom-work-items" &&
                query.get("group_by") === "STATES"
              );
            }).length,
          { timeout: 60_000 }
        )
        .toBeGreaterThan(0);
      expect(await driver.analyticsCustomPickerLabels()).toEqual({
        metric: "Work item",
        xAxis: "Priority",
        split: "State name",
      });
      expect(await driver.analyticsCustomPickerOptions(1)).toEqual(
        X_AXIS_LABELS.filter((label) => label !== "State name")
      );
    });

    await test.step("moving x re-queries and swaps the exclusion", async () => {
      await driver.analyticsCustomPickerSelect(1, "Label");
      await expect
        .poll(
          async () =>
            (await driver.analyticsSpyRequests()).filter((request) => {
              const query = new URLSearchParams(request.query);
              return (
                request.path.replace(/\/$/, "").endsWith("/advance-analytics-charts") &&
                query.get("type") === "custom-work-items" &&
                query.get("x_axis") === "LABELS"
              );
            }).length,
          { timeout: 60_000 }
        )
        .toBeGreaterThan(0);
      expect(await driver.analyticsCustomPickerOptions(2)).toEqual([
        "No value",
        ...X_AXIS_LABELS.filter((label) => label !== "Label"),
      ]);
    });

    await test.step("clearing the split re-queries ungrouped and restores the x picker", async () => {
      await driver.analyticsCustomPickerSelect(2, "No value");
      await expect
        .poll(
          async () =>
            (await driver.analyticsSpyRequests()).filter((request) => {
              const query = new URLSearchParams(request.query);
              return (
                request.path.replace(/\/$/, "").endsWith("/advance-analytics-charts") &&
                query.get("type") === "custom-work-items" &&
                query.get("x_axis") === "LABELS" &&
                query.get("group_by") === null
              );
            }).length,
          { timeout: 60_000 }
        )
        .toBeGreaterThan(0);
      await driver.analyticsStopRequestSpy();
      expect(await driver.analyticsCustomPickerLabels()).toEqual({
        metric: "Work item",
        xAxis: "Label",
        split: "Add Property",
      });
      expect(await driver.analyticsCustomPickerOptions(1)).toEqual(X_AXIS_LABELS);
    });

    await test.step("the server answers the final axis choice with matching rows", async () => {
      const read = await analyticsAdvanceCustomChartsRaw(world.owner, world.workspaceSlug, {
        x_axis: "LABELS",
        y_axis: "WORK_ITEM_COUNT",
      });
      expect(read.status).toBe(200);
      const rows = customRowsOf(read.payload);
      expect(rows).toHaveLength(1);
      expect(rows[0]).toMatchObject({ key: "None", count: 3 });
    });
  }
);

test(
  specTitle(ROW_011, "single-series bars use priority hues and state colors with edge-rounded ends and no legend"),
  { tag: specTags(ROW_011) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl011");
    const project = await setupAnalyticsProject(world, `Anl Hues ${world.tag.slice(-4)}`, "AH");
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "hue urgent",
      { priority: "urgent" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "hue high",
      { priority: "high" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "hue medium",
      { priority: "medium" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "hue low",
      { priority: "low" },
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "hue none", {}, world.owner.apiBase);
    const states = await projectStates(world.workspaceSlug, project.id, world.owner.cookie, world.owner.apiBase);
    const stateIdOf = (group: string): string => {
      const id = states.find((s) => s.group === group)?.id;
      if (!id) throw new Error(`[parity] seeded project has no ${group} state.`);
      return id;
    };

    await enterAnalyticsTab(driver, world, "work-items");

    await test.step("each priority bar fills with its fixed hue and rounds both ends", async () => {
      await expect
        .poll(async () => (await driver.analyticsCustomChartFacts()).segments.length, { timeout: 60_000 })
        .toBe(5);
      const facts = await driver.analyticsCustomChartFacts();
      expect(facts.legend).toBe(false);
      expect(new Set(facts.xLabels)).toEqual(new Set(["Urgent", "High", "Medium", "Low", "None"]));
      const byBar = [...facts.segments].sort((a, b) => a.cx - b.cx);
      expect(byBar).toHaveLength(5);
      const fillsByLabel = new Map(facts.xLabels.map((label, index) => [label, byBar[index]?.fill.toLowerCase()]));
      for (const [label, hue] of Object.entries(PRIORITY_HUES)) {
        expect(fillsByLabel.get(label)).toBe(hue);
      }
      for (const segment of byBar) {
        expect(segment.topRounded).toBe(true);
        expect(segment.bottomRounded).toBe(true);
      }
    });

    await test.step("the server rows carry the lowercase priority keys behind the labels", async () => {
      const read = await analyticsAdvanceCustomChartsRaw(world.owner, world.workspaceSlug, {
        x_axis: "PRIORITY",
        y_axis: "WORK_ITEM_COUNT",
      });
      expect(read.status).toBe(200);
      expect(customSchemaOf(read.payload)).toEqual({});
      expect(new Set(customRowsOf(read.payload).map((row) => row.key))).toEqual(
        new Set(["urgent", "high", "medium", "low", "none"])
      );
    });

    await test.step("state axes reuse the workspace state colors", async () => {
      // Spread the issues so three states plot.
      const moved = await createIssue(
        world.workspaceSlug,
        project.id,
        world.owner.cookie,
        "hue started",
        {},
        world.owner.apiBase
      );
      const done = await createIssue(
        world.workspaceSlug,
        project.id,
        world.owner.cookie,
        "hue done",
        {},
        world.owner.apiBase
      );
      await moveSeededIssue(world, project.id, moved.id, stateIdOf("started"));
      await moveSeededIssue(world, project.id, done.id, stateIdOf("completed"));
      await driver.analyticsCustomPickerSelect(1, "State name");
      const colors = await analyticsProjectStateColors(world.owner, world.workspaceSlug, project.id);
      const colorByName = new Map(colors.map((state) => [state.name, state.color.toLowerCase()]));
      // Ticks land before the bar-growth animation finishes (zero-height
      // segments render nothing), so the settle proof pairs both halves.
      await expect
        .poll(
          async () => {
            const settled = await driver.analyticsCustomChartFacts();
            return settled.xLabels.length >= 3 && settled.segments.length === settled.xLabels.length;
          },
          { timeout: 60_000 }
        )
        .toBe(true);
      const facts = await driver.analyticsCustomChartFacts();
      const byBar = [...facts.segments].sort((a, b) => a.cx - b.cx);
      expect(byBar).toHaveLength(facts.xLabels.length);
      facts.xLabels.forEach((label, index) => {
        expect(byBar[index]?.fill.toLowerCase()).toBe(colorByName.get(label));
      });
    });
  }
);

test(
  specTitle(ROW_011, "date axes group by day with year-aware labels"),
  { tag: specTags(ROW_011) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl011d");
    const project = await setupAnalyticsProject(world, `Anl Dates ${world.tag.slice(-4)}`, "AD");
    const today = new Date().toISOString().slice(0, 10);
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "date past",
      { target_date: "2024-03-15" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "date today",
      { target_date: today },
      world.owner.apiBase
    );
    await createIssue(world.workspaceSlug, project.id, world.owner.cookie, "date none", {}, world.owner.apiBase);

    await enterAnalyticsTab(driver, world, "work-items");
    await driver.analyticsCustomPickerSelect(1, "Due date");

    // Day grouping always: past years print with the year, the current year
    // without it, and missing dates read as None. No grouping switcher
    // renders anywhere in the section (see the ANL-011 row note).
    const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const todayLabel = `${months[Number(today.slice(5, 7)) - 1]} ${Number(today.slice(8, 10))}`;
    await expect
      .poll(async () => (await driver.analyticsCustomChartFacts()).xLabels, { timeout: 60_000 })
      .toEqual(expect.arrayContaining(["Mar 15, 2024", todayLabel, "None"]));
    const facts = await driver.analyticsCustomChartFacts();
    expect(facts.xLabels).toHaveLength(3);

    await test.step("the table under the chart repeats the grouped labels", async () => {
      await expect.poll(() => driver.analyticsExportRowCount("chart"), { timeout: 60_000 }).toBe(3);
      expect(await driver.analyticsExportRowCells("chart")).toEqual(
        expect.arrayContaining([
          expect.arrayContaining(["Mar 15, 2024"]),
          expect.arrayContaining([todayLabel]),
          expect.arrayContaining(["None"]),
        ])
      );
    });
  }
);

test(
  specTitle(ROW_011, "a second split stacks palette segments with edge-only rounding and tooltip identities"),
  { tag: specTags(ROW_011) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl011s");
    const project = await setupAnalyticsProject(world, `Anl Stack ${world.tag.slice(-4)}`, "AS");
    const states = await projectStates(world.workspaceSlug, project.id, world.owner.cookie, world.owner.apiBase);
    const stateIdOf = (group: string): string => {
      const id = states.find((s) => s.group === group)?.id;
      if (!id) throw new Error(`[parity] seeded project has no ${group} state.`);
      return id;
    };
    const first = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "stack one",
      { priority: "urgent" },
      world.owner.apiBase
    );
    const second = await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "stack two",
      { priority: "urgent" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "stack three",
      { priority: "urgent" },
      world.owner.apiBase
    );
    await moveSeededIssue(world, project.id, first.id, stateIdOf("started"));
    await moveSeededIssue(world, project.id, second.id, stateIdOf("completed"));
    // The third stays in the default state, so one bar stacks three segments.

    await enterAnalyticsTab(driver, world, "work-items");
    await driver.analyticsCustomPickerSelect(2, "State name");

    const read = await analyticsAdvanceCustomChartsRaw(world.owner, world.workspaceSlug, {
      x_axis: "PRIORITY",
      y_axis: "WORK_ITEM_COUNT",
      group_by: "STATES",
    });
    expect(read.status).toBe(200);
    const schema = customSchemaOf(read.payload);
    const schemaKeys = Object.keys(schema);
    expect(schemaKeys).toHaveLength(3);

    await test.step("segments stack in schema order on palette slots", async () => {
      await expect
        .poll(async () => (await driver.analyticsCustomChartFacts()).segments.length, { timeout: 60_000 })
        .toBe(3);
      const facts = await driver.analyticsCustomChartFacts();
      expect(facts.xLabels).toEqual(["Urgent"]);
      expect(facts.legend).toBe(false);
      // Bottom to top follows the schema key order.
      const bottomUp = [...facts.segments].sort((a, b) => b.cy - a.cy);
      expect(bottomUp.map((segment) => segment.fill.toLowerCase())).toEqual(PALETTE_SLOTS.slice(0, 3));
    });

    await test.step("only true segment edges round", async () => {
      const facts = await driver.analyticsCustomChartFacts();
      const bottomUp = [...facts.segments].sort((a, b) => b.cy - a.cy);
      expect({ top: bottomUp[0]?.topRounded, bottom: bottomUp[0]?.bottomRounded }).toEqual({
        top: false,
        bottom: true,
      });
      expect({ top: bottomUp[1]?.topRounded, bottom: bottomUp[1]?.bottomRounded }).toEqual({
        top: false,
        bottom: false,
      });
      expect({ top: bottomUp[2]?.topRounded, bottom: bottomUp[2]?.bottomRounded }).toEqual({
        top: true,
        bottom: false,
      });
    });

    await test.step("hovering the bar names each segment through the tooltip", async () => {
      const rows = await driver.analyticsCustomTooltipSegments(0);
      expect(new Set(rows.map((row) => row.label))).toEqual(new Set(Object.values(schema)));
      for (const row of rows) expect(row.value).toBe("1");
    });
  }
);

test(
  specTitle(ROW_011, "past-ten segments extend the theme palette"),
  { tag: specTags(ROW_011) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl011p");
    const project = await setupAnalyticsProject(world, `Anl Palette ${world.tag.slice(-4)}`, "AL");
    for (let index = 0; index < 11; index += 1) {
      const label = await serverCreateLabel(
        world.workspaceSlug,
        project.id,
        `Anl Label ${index}`,
        "#6172e8",
        world.owner.cookie,
        world.owner.apiBase
      );
      const issue = await createIssue(
        world.workspaceSlug,
        project.id,
        world.owner.cookie,
        `palette ${index}`,
        {},
        world.owner.apiBase
      );
      await serverSetIssueLabels(
        world.workspaceSlug,
        project.id,
        issue.id,
        [label.id],
        world.owner.cookie,
        world.owner.apiBase
      );
    }

    const read = await analyticsAdvanceCustomChartsRaw(world.owner, world.workspaceSlug, {
      x_axis: "PRIORITY",
      y_axis: "WORK_ITEM_COUNT",
      group_by: "LABELS",
    });
    expect(read.status).toBe(200);
    expect(Object.keys(customSchemaOf(read.payload))).toHaveLength(11);

    await enterAnalyticsTab(driver, world, "work-items");
    await driver.analyticsCustomPickerSelect(2, "Label");

    // The first ten segments take the theme slots verbatim; the eleventh is
    // a generated extension color, still a plain hex fill.
    await expect
      .poll(async () => (await driver.analyticsCustomChartFacts()).segments.length, { timeout: 60_000 })
      .toBe(11);
    const facts = await driver.analyticsCustomChartFacts();
    const bottomUp = [...facts.segments].sort((a, b) => b.cy - a.cy);
    expect(bottomUp.slice(0, 10).map((segment) => segment.fill.toLowerCase())).toEqual(PALETTE_SLOTS);
    const extended = (bottomUp[10]?.fill ?? "").toLowerCase();
    expect(extended).toMatch(/^#[0-9a-f]{6}$/);
    expect(PALETTE_SLOTS).not.toContain(extended);
  }
);

test(
  specTitle(ROW_012, "the chart table repeats plotted values with search and a filtered export"),
  { tag: specTags(ROW_012) },
  async ({ driver }) => {
    test.setTimeout(720_000);
    const world = await setupAnalyticsWorld("parity-anl012");
    const project = await setupAnalyticsProject(world, `Anl Chart Tab ${world.tag.slice(-4)}`, "AC");
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "ct urgent",
      { priority: "urgent" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "ct high",
      { priority: "high" },
      world.owner.apiBase
    );
    await createIssue(
      world.workspaceSlug,
      project.id,
      world.owner.cookie,
      "ct low",
      { priority: "low" },
      world.owner.apiBase
    );

    await enterAnalyticsTab(driver, world, "work-items");

    await test.step("headers and rows repeat the plotted values", async () => {
      // Settle polls count atomically (multi-call cell reads race row-count
      // shrinkage), then read once.
      await expect.poll(() => driver.analyticsExportRowCount("chart"), { timeout: 60_000 }).toBe(3);
      expect(await driver.analyticsExportHeaders("chart")).toEqual(["Priority", "Count"]);
      expect(await driver.analyticsExportCountLabel("chart")).toBe("3 Priority");
      const read = await analyticsAdvanceCustomChartsRaw(world.owner, world.workspaceSlug, {
        x_axis: "PRIORITY",
        y_axis: "WORK_ITEM_COUNT",
      });
      expect(read.status).toBe(200);
      // Rendered names capitalize the server keys; counts agree.
      const rows = customRowsOf(read.payload);
      const cells = await driver.analyticsExportRowCells("chart");
      expect(new Set(cells.map((row) => row[0]))).toEqual(
        new Set(rows.map((row) => row.name.slice(0, 1).toUpperCase() + row.name.slice(1)))
      );
      for (const cell of cells) {
        const server = rows.find((row) => row.name.slice(0, 1).toUpperCase() + row.name.slice(1) === cell[0]);
        expect(cell[1]).toBe(String(server?.count));
      }
    });

    await test.step("search narrows the table to the matching first column", async () => {
      await driver.analyticsExportSearch("chart", "urgent");
      await expect.poll(() => driver.analyticsExportRowCount("chart"), { timeout: 30_000 }).toBe(1);
      expect(await driver.analyticsExportRowCells("chart")).toEqual([["Urgent", "1"]]);
    });

    await test.step("export downloads the filtered rows under display headers", async () => {
      const file = await driver.analyticsExportDownload("chart");
      expect(file.fileName).toBe(`${world.workspaceSlug}-analytics.csv`);
      const lines = file.content
        .replace(/^﻿/, "")
        .split("\r\n")
        .filter((line) => line !== "")
        .map((line) => line.split(",").map((cell) => cell.replace(/^"|"$/g, "")));
      expect(lines[0]).toEqual(["Priority", "Count"]);
      expect(lines.slice(1)).toEqual([["Urgent", "1"]]);
      await driver.analyticsExportClearSearch("chart");
      await expect.poll(() => driver.analyticsExportRowCount("chart"), { timeout: 30_000 }).toBe(3);
    });
  }
);
