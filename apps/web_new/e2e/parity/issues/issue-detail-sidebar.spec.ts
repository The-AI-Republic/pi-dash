// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios: issue detail sidebar (NEWFRONT-121, Part A). Property
// rows on the full page: state, assignees, runs-on, priority, created-by,
// dates, conditional rows, parent, and labels — against scenario-owned
// issues, green on apps/web first.
// Rows: ISS-148, ISS-149, ISS-150, ISS-151, ISS-152, ISS-153, ISS-154,
// ISS-155, ISS-156, ISS-157, ISS-158, ISS-159.
import { test, expect } from "../fixtures";
import {
  createCycle,
  createModule,
  createProject,
  createProjectLabel,
  createState,
  deleteCycle,
  deleteModule,
  deleteProject,
  deleteProjectLabel,
  deleteState,
  fetchIssue,
  patchIssue,
  patchProject,
  projectFacts,
  projectLabels,
  projectStates,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

test(specTitle(["ISS-148"], "sidebar state property"), { tag: specTags(["ISS-148"]) }, async ({ driver, seed }) => {
  await signIn(driver, seed);
  const session = await signInSession(seed.email, seed.password);
  const issue = await ownIssue(seed, session, `Oracle state ${Date.now()}`);
  const extra = await createState(
    seed.workspaceSlug,
    seed.projectId,
    session,
    `Oracle started ${Date.now()}`,
    "started"
  );
  try {
    await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
    await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
    await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
    await test.step("pick a state and the server follows", async () => {
      await driver.pickState(extra.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).toContain(extra.name);
      await expect
        .poll(async () => (await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["state_id"], {
          timeout: 30_000,
        })
        .toBe(extra.id);
    });
  } finally {
    const states = await projectStates(seed.workspaceSlug, seed.projectId, session).catch(() => []);
    const todo = states.find((row) => row.name === "Todo");
    if (todo) await patchIssue(seed.workspaceSlug, seed.projectId, issue.id, session, { state_id: todo.id });
    await dropIssue(seed, session, issue.id);
    await deleteState(seed.workspaceSlug, seed.projectId, extra.id, session).catch(() => {});
  }
});

test(specTitle(["ISS-149"], "sidebar assignees property"), { tag: specTags(["ISS-149"]) }, async ({ driver, seed }) => {
  await signIn(driver, seed);
  const session = await signInSession(seed.email, seed.password);
  const issue = await ownIssue(seed, session, `Oracle assignees ${Date.now()}`);
  try {
    await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
    await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
    await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
    await test.step("empty placeholder first", async () => {
      expect(await driver.sidebarProperty("Assignees")).toContain("Add assignees");
    });
    await test.step("assign the owner and the server follows", async () => {
      await driver.pickAssignee("You");
      await expect.poll(() => driver.sidebarProperty("Assignees"), { timeout: 30_000 }).toContain("Parity Oracle");
      await expect
        .poll(
          async () =>
            ((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["assignee_ids"] as string[]) ??
            [],
          { timeout: 30_000 }
        )
        .not.toHaveLength(0);
    });
  } finally {
    await dropIssue(seed, session, issue.id);
  }
});

test(specTitle(["ISS-150"], "sidebar runs-on property"), { tag: specTags(["ISS-150"]) }, async ({ driver, seed }) => {
  await signIn(driver, seed);
  const session = await signInSession(seed.email, seed.password);
  const issue = await ownIssue(seed, session, `Oracle runson ${Date.now()}`);
  try {
    await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
    await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
    await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
    expect(await driver.sidebarProperty("Runs on")).toContain("PAR_pod_1");
    const options = await driver.runsOnOptions();
    expect(options.some((name) => name.includes("PAR_pod_1"))).toBe(true);
    expect(options.some((name) => /unavailable|not enabled/i.test(name))).toBe(true);
  } finally {
    await dropIssue(seed, session, issue.id);
  }
});

test(specTitle(["ISS-151"], "sidebar priority property"), { tag: specTags(["ISS-151"]) }, async ({ driver, seed }) => {
  await signIn(driver, seed);
  const session = await signInSession(seed.email, seed.password);
  const issue = await ownIssue(seed, session, `Oracle priority ${Date.now()}`);
  try {
    await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
    await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
    await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
    expect(await driver.sidebarProperty("Priority")).toContain("None");
    await driver.pickPriority("High");
    await expect.poll(() => driver.sidebarProperty("Priority"), { timeout: 30_000 }).toContain("High");
    await expect
      .poll(async () => (await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["priority"], {
        timeout: 30_000,
      })
      .toBe("high");
    await patchIssue(seed.workspaceSlug, seed.projectId, issue.id, session, { priority: "none" });
  } finally {
    await dropIssue(seed, session, issue.id);
  }
});

test(
  specTitle(["ISS-152", "ISS-154"], "creator row and estimate hidden without estimates"),
  { tag: specTags(["ISS-152", "ISS-154"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle rows ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("the creator shows and carries no editing control", async () => {
        expect(await driver.sidebarProperty("Created by")).toContain("Parity Oracle");
        expect(await driver.sidebarRowHasControl("Created by")).toBe(false);
        expect(await driver.sidebarRowHasControl("State")).toBe(true);
      });
      await test.step("estimate stays hidden while the project has estimates off", async () => {
        expect(await driver.sidebarRowPresent("Estimate")).toBe(false);
        const { record } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
        expect(record["estimate"] ?? null).toBeNull();
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-155", "ISS-156"], "module and cycle rows follow the project views"),
  { tag: specTags(["ISS-155", "ISS-156"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    // The seed project's views are shared mutable state (siblings flip
    // them), so both halves run on a scenario-owned scratch project.
    const tag = `${String(Date.now()).slice(-6)}${Math.floor(Math.random() * 90 + 10)}`;
    const scratch = await createProject(seed.workspaceSlug, session, `Oracle scratch ${tag}`, `OZ${tag}`);
    try {
      const { record } = await projectFacts(seed.workspaceSlug, scratch.id, session);
      expect(record["module_view"]).toBe(false);
      expect(record["cycle_view"]).toBe(false);
      const bare = await ownIssue(seed, session, `Oracle bare ${tag}`, {}, scratch.id);
      try {
        await test.step("rows hide with the views off", async () => {
          await driver.openIssueDetail(seed.workspaceSlug, bare.seq);
          await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(bare.name);
          await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
          expect(await driver.sidebarRowPresent("Modules")).toBe(false);
          expect(await driver.sidebarRowPresent("Cycle")).toBe(false);
          expect(await driver.sidebarRowPresent("Estimate")).toBe(false);
        });
        await test.step("rows show once the views turn on", async () => {
          await patchProject(seed.workspaceSlug, scratch.id, session, { module_view: true, cycle_view: true });
          await driver.openIssueDetail(seed.workspaceSlug, bare.seq);
          await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(bare.name);
          await expect.poll(() => driver.sidebarProperty("Cycle"), { timeout: 30_000 }).not.toBeNull();
          expect(await driver.sidebarRowPresent("Modules")).toBe(true);
          expect(await driver.sidebarProperty("Modules")).toContain("No module");
          expect(await driver.sidebarProperty("Cycle")).toContain("No cycle");
          const { record: on } = await projectFacts(seed.workspaceSlug, scratch.id, session);
          expect(on["module_view"]).toBe(true);
          expect(on["cycle_view"]).toBe(true);
        });
      } finally {
        await dropIssue(seed, session, bare.id, scratch.id);
      }
    } finally {
      await deleteProject(seed.workspaceSlug, scratch.id, session);
    }
  }
);

test(
  specTitle(["ISS-155"], "module assignment diffs into add and remove sets"),
  { tag: specTags(["ISS-155"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const tag = `${String(ts).slice(-6)}${Math.floor(Math.random() * 90 + 10)}`;
    const scratch = await createProject(seed.workspaceSlug, session, `Oracle modproj ${tag}`, `OM${tag}`);
    try {
      await patchProject(seed.workspaceSlug, scratch.id, session, { module_view: true });
      const module = await createModule(seed.workspaceSlug, scratch.id, session, `Oracle mod ${ts}`);
      const issue = await ownIssue(seed, session, `Oracle mod issue ${ts}`, {}, scratch.id);
      try {
        await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
        await expect.poll(() => driver.sidebarProperty("Modules"), { timeout: 30_000 }).not.toBeNull();
        await test.step("selecting a module adds the bridge", async () => {
          await driver.toggleModule(module.name);
          await expect
            .poll(
              async () =>
                (await fetchIssue(seed.workspaceSlug, scratch.id, issue.id, session))["module_ids"] as string[],
              { timeout: 30_000 }
            )
            .toContain(module.id);
          expect(await driver.sidebarProperty("Modules")).toContain(module.name);
        });
        await test.step("toggling again removes the bridge", async () => {
          await driver.toggleModule(module.name);
          await expect
            .poll(
              async () =>
                (await fetchIssue(seed.workspaceSlug, scratch.id, issue.id, session))["module_ids"] as string[],
              { timeout: 30_000 }
            )
            .toEqual([]);
          expect(await driver.sidebarProperty("Modules")).toContain("No module");
        });
      } finally {
        await dropIssue(seed, session, issue.id, scratch.id);
        await deleteModule(seed.workspaceSlug, scratch.id, module.id, session);
      }
    } finally {
      await deleteProject(seed.workspaceSlug, scratch.id, session);
    }
  }
);

test(
  specTitle(["ISS-156"], "cycle assignment adds, reselect no-ops, clearing removes"),
  { tag: specTags(["ISS-156"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const tag = `${String(ts).slice(-6)}${Math.floor(Math.random() * 90 + 10)}`;
    const scratch = await createProject(seed.workspaceSlug, session, `Oracle cycproj ${tag}`, `OC${tag}`);
    try {
      await patchProject(seed.workspaceSlug, scratch.id, session, { cycle_view: true });
      const cycle = await createCycle(seed.workspaceSlug, scratch.id, session, `Oracle cyc ${ts}`);
      const issue = await ownIssue(seed, session, `Oracle cyc issue ${ts}`, {}, scratch.id);
      try {
        await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
        await expect.poll(() => driver.sidebarProperty("Cycle"), { timeout: 30_000 }).not.toBeNull();
        await test.step("picking a cycle assigns the issue", async () => {
          await driver.pickCycle(cycle.name);
          await expect
            .poll(async () => (await fetchIssue(seed.workspaceSlug, scratch.id, issue.id, session))["cycle_id"], {
              timeout: 30_000,
            })
            .toBe(cycle.id);
          expect(await driver.sidebarProperty("Cycle")).toContain(cycle.name);
        });
        await test.step("reselecting the same cycle changes nothing", async () => {
          await driver.pickCycle(cycle.name);
          expect(await driver.sidebarProperty("Cycle")).toContain(cycle.name);
          expect((await fetchIssue(seed.workspaceSlug, scratch.id, issue.id, session))["cycle_id"]).toBe(cycle.id);
        });
        await test.step("clearing removes the bridge with a toast", async () => {
          await driver.clearCycle();
          await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("removed from the cycle");
          await expect
            .poll(async () => (await fetchIssue(seed.workspaceSlug, scratch.id, issue.id, session))["cycle_id"], {
              timeout: 30_000,
            })
            .toBeNull();
          expect(await driver.sidebarProperty("Cycle")).toContain("No cycle");
        });
      } finally {
        await dropIssue(seed, session, issue.id, scratch.id);
        await deleteCycle(seed.workspaceSlug, scratch.id, cycle.id, session);
      }
    } finally {
      await deleteProject(seed.workspaceSlug, scratch.id, session);
    }
  }
);

test(
  specTitle(["ISS-153"], "sidebar start and due dates"),
  { tag: specTags(["ISS-153"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle dates ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("pick both dates and the server follows", async () => {
        await driver.pickDate("Start date", "15");
        await expect
          .poll(
            async () =>
              String((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["start_date"] ?? ""),
            { timeout: 30_000 }
          )
          .toContain("-15");
        expect(await driver.sidebarProperty("Start date")).not.toContain("Add start date");
        await driver.pickDate("Due date", "20");
        await expect
          .poll(
            async () =>
              String((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["target_date"] ?? ""),
            { timeout: 30_000 }
          )
          .toContain("-20");
      });
      await test.step("clearing sets null again", async () => {
        await driver.clearDate("Start date");
        await expect
          .poll(async () => (await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["start_date"], {
            timeout: 30_000,
          })
          .toBeNull();
        expect(await driver.sidebarProperty("Start date")).toContain("Add start date");
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-157", "ISS-158"], "parent work item set, banner, and remove"),
  { tag: specTags(["ISS-157", "ISS-158"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const child = await ownIssue(seed, session, `Oracle child ${ts}`);
    const parent = await ownIssue(seed, session, `Oracle parent ${ts}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, child.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(child.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("set the parent and the server follows", async () => {
        await driver.setParentByName(parent.name);
        await expect
          .poll(async () => (await fetchIssue(seed.workspaceSlug, seed.projectId, child.id, session))["parent_id"], {
            timeout: 30_000,
          })
          .toBe(parent.id);
        await expect.poll(() => driver.sidebarProperty("Parent"), { timeout: 30_000 }).toContain(parent.seq);
      });
      await test.step("banner names the parent and links to it", async () => {
        await expect.poll(() => driver.parentBanner(child.seq), { timeout: 30_000 }).toContain(parent.seq);
        expect(await driver.parentBanner(child.seq)).toContain(parent.name);
        await driver.openParentFromBanner();
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 60_000 }).toBe(parent.name);
        await driver.openIssueDetail(seed.workspaceSlug, child.seq);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 60_000 }).toBe(child.name);
      });
      await test.step("banner menu lists the remove item; remove clears it", async () => {
        const names = await driver.bannerMenuNames(child.seq);
        expect(names.some((entry) => /Remove parent work item/i.test(entry))).toBe(true);
        await driver.page.keyboard.press("Escape");
        await driver.removeParent();
        await expect
          .poll(async () => (await fetchIssue(seed.workspaceSlug, seed.projectId, child.id, session))["parent_id"], {
            timeout: 30_000,
          })
          .toBeNull();
        expect(await driver.parentBanner(child.seq)).toBeNull();
      });
    } finally {
      await dropIssue(seed, session, child.id);
      await dropIssue(seed, session, parent.id);
    }
  }
);

test(
  specTitle(["ISS-159"], "sidebar labels add, remove, and inline create"),
  { tag: specTags(["ISS-159"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle labels ${Date.now()}`);
    const existing = await createProjectLabel(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Oracle existing ${Date.now()}`
    );
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("assign an existing label", async () => {
        await driver.addLabel(existing.name);
        await expect
          .poll(
            async () =>
              ((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["label_ids"] as string[]) ??
              [],
            { timeout: 30_000 }
          )
          .toContain(existing.id);
        expect(await driver.sidebarProperty("Labels")).toContain(existing.name);
      });
      await test.step("remove it again", async () => {
        await driver.removeLabel(existing.name);
        await expect
          .poll(
            async () =>
              ((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["label_ids"] as string[]) ??
              [],
            { timeout: 30_000 }
          )
          .not.toContain(existing.id);
      });
      await test.step("inline-create a fresh label by name", async () => {
        const fresh = `Oracle fresh ${Date.now()}`;
        await driver.addLabel(fresh);
        await expect
          .poll(
            async () =>
              (await projectLabels(seed.workspaceSlug, seed.projectId, session)).some((row) => row.name === fresh),
            { timeout: 30_000 }
          )
          .toBe(true);
        const created = (await projectLabels(seed.workspaceSlug, seed.projectId, session)).find(
          (row) => row.name === fresh
        );
        await deleteProjectLabel(seed.workspaceSlug, seed.projectId, created?.id as string, session);
      });
    } finally {
      await deleteProjectLabel(seed.workspaceSlug, seed.projectId, existing.id, session).catch(() => {});
      await dropIssue(seed, session, issue.id);
    }
  }
);
