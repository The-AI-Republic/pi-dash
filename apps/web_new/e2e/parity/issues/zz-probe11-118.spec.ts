// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// TEMPORARY PROBE — NEWFRONT-118 run 4. Capture sub-grouped fetch bodies
// for ISS-031 and card-name reader divergence for ISS-043. Deleted before PR.
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

test("probe11a: capture sub-grouped fetch bodies", async ({ driver, seed, page }) => {
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
        const rec = body as { results?: unknown };
        const results = rec.results as Record<string, unknown> | unknown[] | undefined;
        const summary = Array.isArray(results)
          ? `array[${results.length}]:${JSON.stringify(results.map((r) => (r as { name?: string }).name)).slice(0, 200)}`
          : `keys:${JSON.stringify(Object.keys(results ?? {})).slice(0, 300)}`;
        console.log(`P11A-FETCH:${url.slice(-140)} => ${summary.slice(0, 300)}`);
      })
      .catch(() => console.log(`P11A-FETCH:${url.slice(-100)} => non-json ${res.status()}`));
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
  await page.waitForTimeout(20_000);
  console.log(`P11A-LANES:${JSON.stringify(await driver.kanbanSwimlanes())}`);
  console.log(`P11A-CARDS:${JSON.stringify((await driver.kanbanCards()).map((c) => c.name))}`);
  await serverPatchIssue(seed.workspaceSlug, seed.projectId, first.id, { label_ids: [] }, owner.cookie);
  await serverDeleteLabel(seed.workspaceSlug, seed.projectId, label.id, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, seed.projectId, user.cookie, {
    display_filters: before.displayFilters,
    display_properties: before.displayProperties,
  });
  console.log("P11A-DONE");
});

test("probe11b: card name textContent vs innerText", async ({ driver, seed, page }) => {
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
  const before = await serverProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie);
  await serverPatchProjectUserProperties(seed.workspaceSlug, projectId, owner.cookie, {
    display_filters: { ...before.displayFilters, layout: "kanban", order_by: "sort_order", group_by: "state" },
  });
  await driver.openAuthenticated(`/${seed.workspaceSlug}/projects/${projectId}/issues`, browserCookies(owner));
  await driver.kanbanOpenBoard();
  await driver.kanbanColumnScrollEnd(home.name);
  await page.waitForTimeout(5_000);
  const dump = await page.evaluate(() => {
    const anchors = [...document.querySelectorAll('a[id^="issue_"]')];
    const last = anchors[anchors.length - 1];
    if (!(last instanceof HTMLElement)) return { anchors: anchors.length, last: "none" };
    const span = last.querySelector("div.text-body-sm-medium > span");
    const cs = span ? getComputedStyle(span) : null;
    return {
      anchors: anchors.length,
      id: last.id,
      textContent: JSON.stringify((span?.textContent ?? "NO-SPAN").slice(0, 60)),
      innerText: JSON.stringify(((span as HTMLElement | null)?.innerText ?? "NO-SPAN").slice(0, 60)),
      box: JSON.stringify(last.getBoundingClientRect().toJSON()),
      whiteSpace: cs?.whiteSpace ?? "?",
      overflow: cs?.overflow ?? "?",
      lineClamp: (cs as unknown as Record<string, string>)?.webkitLineClamp ?? "?",
      display: cs?.display ?? "?",
    };
  });
  console.log(`P11B-DUMP:${JSON.stringify(dump)}`);
  await serverDeleteProject(seed.workspaceSlug, projectId, owner.cookie);
  console.log("P11B-DONE");
});
