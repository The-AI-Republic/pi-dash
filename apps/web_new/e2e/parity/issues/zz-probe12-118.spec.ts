// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 4. Deep grouped-response dump for
// ISS-031; skeleton-vs-shell sampling for ISS-043. Deleted before PR.
import { test } from "../fixtures";
import {
  browserCookies,
  serverCreateIssue,
  serverCreateLabel,
  serverCreateProject,
  serverDeleteLabel,
  serverDeleteProject,
  serverIssues,
  serverListStates,
  serverPatchIssue,
  serverPatchProjectUserProperties,
  serverProjectUserProperties,
  signInFreshUser,
  uniqueSuffix,
} from "../helpers/api";

test("probe12a: deep grouped response", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6);
  const label = await serverCreateLabel(seed.workspaceSlug, seed.projectId, owner.cookie, `KB fold ${suffix}`);
  const rows = await serverIssues(seed.workspaceSlug, seed.projectId, owner.cookie);
  const first = rows.find((r) => r.name === (seed.issueNames[0] ?? ""));
  if (!first) throw new Error("[probe] no seed issue");
  await serverPatchIssue(seed.workspaceSlug, seed.projectId, first.id, { label_ids: [label.id] }, owner.cookie);
  page.on("response", (res) => {
    const url = res.url();
    if (!url.includes("/issues/") || !url.includes("sub_group_by")) return;
    void res
      .json()
      .then((body: unknown) => {
        const results = (body as { results?: Record<string, unknown> }).results ?? {};
        const deep: Record<string, unknown> = {};
        for (const [group, sub] of Object.entries(results)) {
          if (Array.isArray(sub)) {
            deep[group] = `array[${sub.length}]`;
          } else if (sub && typeof sub === "object") {
            const inner: Record<string, unknown> = {};
            for (const [k, v] of Object.entries(sub as Record<string, unknown>)) {
              inner[k.slice(0, 8)] = Array.isArray(v) ? v.map((r) => (r as { name?: string }).name ?? "?") : typeof v;
            }
            deep[group.slice(0, 8)] = inner;
          } else {
            deep[group.slice(0, 8)] = typeof sub;
          }
        }
        console.log(`P12A-FETCH:${url.slice(-80)} => ${JSON.stringify(deep).slice(0, 600)}`);
      })
      .catch(() => console.log(`P12A-FETCH:non-json ${res.status()}`));
  });
  const user = await signInFreshUser(seed.email, seed.password);
  const before = await serverProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: {
      ...before.displayFilters,
      layout: "kanban",
      order_by: "sort_order",
      group_by: "state",
      sub_group_by: "labels",
      show_empty_groups: true,
    },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${seed.projectId}/issues`, browserCookies(user));
  await driver.kanbanOpenBoard();
  await page.waitForTimeout(15_000);
  console.log(`P12A-LANES:${JSON.stringify(await driver.kanbanSwimlanes())}`);
  console.log(`P12A-CARDS:${JSON.stringify((await driver.kanbanCards()).map((c) => c.name))}`);
  await serverPatchIssue(seed.workspaceSlug, seed.projectId, first.id, { label_ids: [] }, owner.cookie);
  await serverDeleteLabel(seed.workspaceSlug, seed.projectId, label.id, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: before.displayFilters,
    display_properties: before.displayProperties,
  });
  console.log("P12A-DONE");
});

test("probe12b: shell content sampling", async ({ driver, seed, page }) => {
  test.setTimeout(480_000);
  const owner = await signInFreshUser(seed.email, seed.password);
  const suffix = uniqueSuffix().slice(0, 6).toUpperCase();
  const projectId = await serverCreateProject(seed.workspaceSlug, owner.cookie, `KB probe ${suffix}`, `KQ${suffix}`);
  const states = await serverListStates(seed.workspaceSlug, projectId, owner.cookie);
  const home = states.find((s) => s.isDefault) ?? states[0];
  if (!home) throw new Error("[probe] no states");
  for (let i = 0; i < 40; i += 1) {
    await serverCreateIssue(seed.workspaceSlug, projectId, owner.cookie, `KB probe ${suffix} ${i + 1}`, home.id);
  }
  const serverRows = await serverIssues(seed.workspaceSlug, projectId, owner.cookie);
  const serverIds = new Set(serverRows.map((r) => r.id));
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie, {
    display_filters: { ...before.displayFilters, layout: "kanban", order_by: "sort_order", group_by: "state" },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(owner));
  await driver.kanbanOpenBoard();
  const sample = () =>
    page.evaluate(
      (ids: string[]) => {
        const anchors = [...document.querySelectorAll('a[id^="issue_"]')];
        let withSpan = 0;
        let realId = 0;
        let lastHtml = "";
        for (const a of anchors) {
          if (a.querySelector("div.text-body-sm-medium > span")) withSpan += 1;
          const parts = (a.id ?? "").split("_");
          if (ids.includes(parts[1] ?? "")) realId += 1;
        }
        const last = anchors[anchors.length - 1];
        if (last instanceof HTMLElement) lastHtml = last.innerHTML.slice(0, 300).replace(/\s+/g, " ");
        return { anchors: anchors.length, withSpan, realId, lastHtml };
      },
      [...serverIds]
    );
  console.log(`P12B-REST:${JSON.stringify(await sample())}`);
  await driver.kanbanColumnScrollEnd(home.name);
  for (let i = 0; i < 6; i += 1) {
    await page.waitForTimeout(5_000);
    console.log(`P12B-T${i}:${JSON.stringify(await sample())}`);
  }
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
  console.log("P12B-DONE");
});
