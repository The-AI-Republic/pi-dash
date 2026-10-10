// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-178): runners overview, pods, row actions.
// Rows: RUN-001/005, RUN-010/015. File order is load-bearing: the
// status-badge scenario runs before any scenario that creates agent
// runs, so dispatch can never flip a seeded badge mid-suite on a stack
// with a celery consumer.
import { test, expect } from "../fixtures";
import {
  createIssue,
  createPod,
  deleteIssue,
  deletePod,
  parityProjectIdentifier,
  projectFacts,
  recentRuns,
  requireGuestSeed,
  serverCancelRun,
  serverCreateProjectWithFlags,
  serverCreateRun,
  serverCreateRunnerV1,
  serverDeleteApiToken,
  serverDeletePodResult,
  serverDeleteProject,
  serverDeleteRunner,
  serverIssuePodPin,
  serverMintApiToken,
  serverPatchPod,
  serverPods,
  serverRunnerOrNull,
  serverRunners,
  serverUnpinIssuePod,
  serverWorkspaceId,
  signInSession,
  type ParityCreatedRun,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** Seed runner names (seed_parity.py RUNNER_FIXTURES). */
const SEED_RUNNERS = [
  "parity-runner-online",
  "parity-runner-busy",
  "parity-runner-offline",
  "parity-runner-revoked",
  "parity-runner-pending",
];

const stamp = () => Date.now().toString(36);

/**
 * Create a run, tolerating a commit-then-500: the first creation once
 * returned HTML while the row persisted, so on failure this looks for
 * the run the attempt may have committed before rethrowing.
 */
async function createRunOrFind(
  sessionCookie: string,
  input: { workspaceId: string; prompt: string; workItemId: string; podId?: string }
): Promise<ParityCreatedRun> {
  try {
    return await serverCreateRun(sessionCookie, input);
  } catch (error) {
    const runs = await recentRuns(sessionCookie);
    const committed = runs.find((row) => row["work_item"] === input.workItemId && row["prompt"] === input.prompt);
    if (
      typeof committed?.["id"] === "string" &&
      typeof committed?.["status"] === "string" &&
      typeof committed?.["pod"] === "string"
    ) {
      return { id: committed["id"], status: committed["status"], pod: committed["pod"] };
    }
    throw error;
  }
}

test(
  specTitle(["RUN-001"], "guest sees the not-authorized view in workspace scope"),
  { tag: specTags(["RUN-001"]) },
  async ({ driver, seed }) => {
    const guest = requireGuestSeed(seed);

    await test.step("sign in as guest", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(guest.email, guest.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
    });

    await test.step("the denied view renders inside normal chrome", async () => {
      // The waiter resolves on whichever renders post-load (tabs or the
      // gate), so denied-here plus chrome-here pins "inside chrome after
      // role load" rather than a mid-load flash on a bare page.
      expect(await driver.runnersDeniedVisible()).toBe(true);
      expect(await driver.runnersChromePresent()).toBe(true);
    });
  }
);

test(
  specTitle(["RUN-001"], "guest sees the not-authorized view in project scope"),
  { tag: specTags(["RUN-001"]) },
  async ({ driver, seed }) => {
    const guest = requireGuestSeed(seed);

    await test.step("sign in as guest", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(guest.email, guest.password);
    });

    await test.step("open the project runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug, seed.projectId);
    });

    await test.step("the denied view renders inside project chrome", async () => {
      expect(await driver.runnersDeniedVisible()).toBe(true);
      expect(await driver.runnersChromePresent()).toBe(true);
      expect(await driver.runnersBreadcrumbLeaf()).toBe("AI Workers");
    });
  }
);

test(
  specTitle(["RUN-001"], "members reach the area without the denied view"),
  { tag: specTags(["RUN-001"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
    });

    await test.step("the list renders and the gate stays hidden", async () => {
      expect(await driver.runnersDeniedVisible()).toBe(false);
      await expect
        .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
        .toEqual(expect.arrayContaining(SEED_RUNNERS));
    });
  }
);

test(
  specTitle(["RUN-002"], "workspace list shows one server-matching row per runner"),
  { tag: specTags(["RUN-002"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
    });

    await test.step("every seeded runner renders with its columns", async () => {
      await expect
        .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
        .toEqual(expect.arrayContaining(SEED_RUNNERS));
      const rows = await driver.runnersTableRows();
      const online = rows.find((row) => row.name === "parity-runner-online");
      expect(online).toBeDefined();
      expect(online!.status).toBe("online");
      expect(online!.osArch).toBe("linux / arm64");
      expect(online!.version).toBe("0.1.0");
      expect(online!.heartbeat).not.toBe("—");
      expect(online!.heartbeat.length).toBeGreaterThan(0);
      const bare = rows.find((row) => row.name === "parity-runner-offline");
      expect(bare).toBeDefined();
      expect(bare!.osArch).toBe("—");
      expect(bare!.version).toBe("—");
      expect(bare!.heartbeat).toBe("—");
    });

    await test.step("the server agrees with the screen", async () => {
      const session = await signInSession(seed.email, seed.password);
      const workspaceId = await serverWorkspaceId(seed.workspaceSlug, seed.projectId, session);
      const server = await serverRunners(workspaceId, session);
      const visible = await driver.runnersTableRows();
      expect(new Set(visible.map((row) => row.name))).toEqual(new Set(server.map((row) => row.name)));
      for (const row of visible) {
        const match = server.find((entry) => entry.name === row.name);
        expect(match).toBeDefined();
        expect(row.pod).toBe(match!.podName);
        expect(row.status).toBe(match!.status);
      }
    });
  }
);

test(
  specTitle(["RUN-002"], "project scope narrows the list and rewrites every link base"),
  { tag: specTags(["RUN-002"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const workspaceId = await serverWorkspaceId(seed.workspaceSlug, seed.projectId, session);
    const identifier = parityProjectIdentifier("R178");
    const extraId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `runners extra ${stamp()}`,
      identifier,
      {},
      session
    );
    const token = await serverMintApiToken(session, `parity178-scope-${stamp()}`);
    const xr = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, name: `xr178${stamp()}` },
      undefined
    );
    const projectBase = `/${seed.workspaceSlug}/projects/${seed.projectId}/runners`;
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("the project list excludes the other project's runner", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug, seed.projectId);
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
          .toEqual(expect.arrayContaining(SEED_RUNNERS));
        const names = (await driver.runnersTableRows()).map((row) => row.name);
        expect(names).not.toContain(xr.runnerName);
      });

      await test.step("tabs, rail and detail links carry the project base", async () => {
        for (const tab of await driver.runnersTabs()) {
          expect(tab.href).toMatch(new RegExp(`^${projectBase}(/runs|/approvals)?$`));
        }
        const rail = await driver.runnersRail();
        expect(rail.overviewHref).toBe(projectBase);
        for (const contact of rail.contacts) {
          expect(contact.href).toContain(`${projectBase}/chat/`);
        }
        const server = await serverRunners(workspaceId, session, seed.projectId);
        const onlineId = server.find((row) => row.name === "parity-runner-online")!.id;
        await driver.runnersOpenDetails("parity-runner-online");
        expect(await driver.runnersCurrentUrl()).toContain(`${projectBase}/detail/${onlineId}`);
      });

      await test.step("the workspace aggregate still shows every runner", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
          .toContain(xr.runnerName);
      });

      await test.step("the server scopes the same way", async () => {
        const scoped = await serverRunners(workspaceId, session, seed.projectId);
        expect(scoped.map((row) => row.name)).not.toContain(xr.runnerName);
        const all = await serverRunners(workspaceId, session);
        expect(all.map((row) => row.name)).toContain(xr.runnerName);
      });
    } finally {
      await serverDeleteRunner(xr.runnerId, session);
      await serverDeleteApiToken(token.id, session);
      await serverDeleteProject(seed.workspaceSlug, extraId, session);
    }
  }
);

test(
  specTitle(["RUN-002"], "the list re-polls without navigation"),
  { tag: specTags(["RUN-002"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      await expect
        .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
        .toEqual(expect.arrayContaining(SEED_RUNNERS));
    });

    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const token = await serverMintApiToken(session, `parity178-poll-${stamp()}`);
    const urlBefore = await driver.runnersCurrentUrl();
    const created = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, name: `lp178${stamp()}` },
      undefined
    );
    try {
      await test.step("the new runner appears with no navigation", async () => {
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 45_000 })
          .toContain(created.runnerName);
        expect(await driver.runnersCurrentUrl()).toBe(urlBefore);
      });
    } finally {
      await serverDeleteRunner(created.runnerId, session);
      await serverDeleteApiToken(token.id, session);
    }
  }
);

test(
  specTitle(["RUN-002"], "a project with no runners shows the empty state"),
  { tag: specTags(["RUN-002"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const workspaceId = await serverWorkspaceId(seed.workspaceSlug, seed.projectId, session);
    const extraId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `runners empty ${stamp()}`,
      parityProjectIdentifier("R178"),
      {},
      session
    );
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the empty project's runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug, extraId);
      });

      await test.step("the empty state shows and the server agrees", async () => {
        expect(await driver.runnersEmptyVisible()).toBe(true);
        expect(await driver.runnersTableRows()).toEqual([]);
        expect(await serverRunners(workspaceId, session, extraId)).toEqual([]);
      });
    } finally {
      await serverDeleteProject(seed.workspaceSlug, extraId, session);
    }
  }
);

test(
  specTitle(["RUN-003"], "workspace tabs route and track the URL"),
  { tag: specTags(["RUN-003"]) },
  async ({ driver, seed }) => {
    const base = `/${seed.workspaceSlug}/runners`;

    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the overview tab starts active with three routes", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      const tabs = await driver.runnersTabs();
      expect(tabs.map((tab) => tab.label)).toEqual(["Overview", "Runs", "Approvals"]);
      expect(tabs.map((tab) => tab.href)).toEqual([base, `${base}/runs`, `${base}/approvals`]);
      expect(tabs.map((tab) => tab.active)).toEqual([true, false, false]);
    });

    await test.step("opening Runs moves the active marker off Overview", async () => {
      await driver.runnersOpenTab("Runs");
      expect(await driver.runnersCurrentUrl()).toContain(`${base}/runs`);
      const tabs = await driver.runnersTabs();
      expect(tabs.find((tab) => tab.label === "Runs")!.active).toBe(true);
      expect(tabs.find((tab) => tab.label === "Overview")!.active).toBe(false);
    });

    await test.step("the approvals route deep-links its tab", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug, undefined, "approvals");
      expect(await driver.runnersCurrentUrl()).toContain(`${base}/approvals`);
      const tabs = await driver.runnersTabs();
      expect(tabs.find((tab) => tab.label === "Approvals")!.active).toBe(true);
    });

    await test.step("back to the overview root", async () => {
      await driver.runnersOpenTab("Overview");
      const url = new URL(await driver.runnersCurrentUrl());
      expect(url.pathname).toBe(base);
      const tabs = await driver.runnersTabs();
      expect(tabs.find((tab) => tab.label === "Overview")!.active).toBe(true);
    });
  }
);

test(
  specTitle(["RUN-003"], "project tabs carry the project base and deep-link"),
  { tag: specTags(["RUN-003"]) },
  async ({ driver, seed }) => {
    const base = `/${seed.workspaceSlug}/projects/${seed.projectId}/runners`;

    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the project tab strip uses the project base", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug, seed.projectId);
      const tabs = await driver.runnersTabs();
      expect(tabs.map((tab) => tab.href)).toEqual([base, `${base}/runs`, `${base}/approvals`]);
      expect(tabs.find((tab) => tab.label === "Overview")!.active).toBe(true);
    });

    await test.step("the project runs route deep-links its tab", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug, seed.projectId, "runs");
      expect(await driver.runnersCurrentUrl()).toContain(`${base}/runs`);
      const tabs = await driver.runnersTabs();
      expect(tabs.find((tab) => tab.label === "Runs")!.active).toBe(true);
      expect(tabs.find((tab) => tab.label === "Overview")!.active).toBe(false);
    });
  }
);

test(
  specTitle(["RUN-004"], "the rail lists one chat contact per runner"),
  { tag: specTags(["RUN-004"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
    });

    await test.step("header, overview link and dotted contacts render", async () => {
      const session = await signInSession(seed.email, seed.password);
      const workspaceId = await serverWorkspaceId(seed.workspaceSlug, seed.projectId, session);
      const server = await serverRunners(workspaceId, session);
      await expect
        .poll(() => driver.runnersRail().then((rail) => rail.contacts.map((contact) => contact.name)), {
          timeout: 60_000,
        })
        .toEqual(expect.arrayContaining(SEED_RUNNERS));
      const rail = await driver.runnersRail();
      expect(rail.header).toBe("AI Agents");
      expect(rail.overviewHref).toBe(`/${seed.workspaceSlug}/runners`);
      expect(rail.emptyVisible).toBe(false);
      expect(new Set(rail.contacts.map((contact) => contact.name))).toEqual(new Set(server.map((row) => row.name)));
      for (const contact of rail.contacts) {
        const match = server.find((row) => row.name === contact.name);
        expect(match).toBeDefined();
        expect(contact.href).toBe(`/${seed.workspaceSlug}/runners/chat/${match!.id}`);
        expect(contact.hasDot).toBe(true);
      }
    });
  }
);

test(
  specTitle(["RUN-004"], "the rail follows the project scope and its empty state"),
  { tag: specTags(["RUN-004"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const extraId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `runners railempty ${stamp()}`,
      parityProjectIdentifier("R178"),
      {},
      session
    );
    const projectBase = `/${seed.workspaceSlug}/projects/${seed.projectId}/runners`;
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("project contacts link into the project chat tree", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug, seed.projectId);
        await expect
          .poll(() => driver.runnersRail().then((rail) => rail.contacts.map((contact) => contact.name)), {
            timeout: 60_000,
          })
          .toEqual(expect.arrayContaining(SEED_RUNNERS));
        const rail = await driver.runnersRail();
        expect(rail.overviewHref).toBe(projectBase);
        for (const contact of rail.contacts) {
          expect(contact.href).toContain(`${projectBase}/chat/`);
        }
      });

      await test.step("a runnerless project shows the rail empty state", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug, extraId);
        await expect.poll(() => driver.runnersRail().then((rail) => rail.emptyVisible), { timeout: 60_000 }).toBe(true);
        const rail = await driver.runnersRail();
        expect(rail.contacts).toEqual([]);
        expect(rail.overviewHref).toBe(`/${seed.workspaceSlug}/projects/${extraId}/runners`);
      });
    } finally {
      await serverDeleteProject(seed.workspaceSlug, extraId, session);
    }
  }
);

test(
  specTitle(["RUN-004"], "the rail refreshes without navigation"),
  { tag: specTags(["RUN-004"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      await expect
        .poll(() => driver.runnersRail().then((rail) => rail.contacts.map((contact) => contact.name)), {
          timeout: 60_000,
        })
        .toEqual(expect.arrayContaining(SEED_RUNNERS));
    });

    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const token = await serverMintApiToken(session, `parity178-rail-${stamp()}`);
    const urlBefore = await driver.runnersCurrentUrl();
    const created = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, name: `rl178${stamp()}` },
      undefined
    );
    try {
      await test.step("the new contact appears with no navigation", async () => {
        await expect
          .poll(() => driver.runnersRail().then((rail) => rail.contacts.map((contact) => contact.name)), {
            timeout: 45_000,
          })
          .toContain(created.runnerName);
        expect(await driver.runnersCurrentUrl()).toBe(urlBefore);
      });
    } finally {
      await serverDeleteRunner(created.runnerId, session);
      await serverDeleteApiToken(token.id, session);
    }
  }
);

test(
  specTitle(["RUN-005"], "bug: NEWFRONT-191 area naming differs across surfaces"),
  { tag: specTags(["RUN-005"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    let title = "";
    let railHeader = "";
    let headings: string[] = [];
    await test.step("read the workspace surfaces", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      title = await driver.runnersPageTitle();
      railHeader = (await driver.runnersRail()).header;
      headings = await driver.runnersSectionHeadings();
    });

    let leaf: string | null = null;
    await test.step("read the project breadcrumb", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug, seed.projectId);
      leaf = await driver.runnersBreadcrumbLeaf();
    });

    await test.step("the three names disagree (the bug)", async () => {
      // bug: NEWFRONT-191 decides the canonical name; until then the
      // oracle pins the current split (the CLI `runner` verb is the
      // fourth witness, recorded on the follow-up, not drivable here).
      expect(title).toContain("AI Agents");
      expect(railHeader).toBe("AI Agents");
      expect(headings).toEqual(expect.arrayContaining(["Pods", "Runners"]));
      expect(leaf).toBe("AI Workers");
      expect(new Set(["AI Agents", "AI Workers", "Runners"]).size).toBe(3);
    });
  }
);

test(
  specTitle(["RUN-010"], "row Details links open the runner detail in both scopes"),
  { tag: specTags(["RUN-010"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const workspaceId = await serverWorkspaceId(seed.workspaceSlug, seed.projectId, session);
    const server = await serverRunners(workspaceId, session);
    const onlineId = server.find((row) => row.name === "parity-runner-online")!.id;

    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("workspace detail link", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      expect((await driver.runnersRowActions("parity-runner-online")).hasDetails).toBe(true);
      await driver.runnersOpenDetails("parity-runner-online");
      expect(await driver.runnersCurrentUrl()).toContain(`/${seed.workspaceSlug}/runners/detail/${onlineId}`);
    });

    await test.step("project detail link", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug, seed.projectId);
      await driver.runnersOpenDetails("parity-runner-online");
      expect(await driver.runnersCurrentUrl()).toContain(
        `/${seed.workspaceSlug}/projects/${seed.projectId}/runners/detail/${onlineId}`
      );
    });
  }
);

test(
  specTitle(["RUN-010"], "revoke shows only for enrolled runners and keeps the row"),
  { tag: specTags(["RUN-010"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      await expect
        .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
        .toEqual(expect.arrayContaining(SEED_RUNNERS));
    });

    await test.step("revoke visibility follows enrolment and state", async () => {
      expect(await driver.runnersRowActions("parity-runner-online")).toEqual({
        hasDetails: true,
        hasRevoke: true,
        hasDelete: true,
      });
      // Added but never enrolled: no credentials to invalidate.
      expect((await driver.runnersRowActions("parity-runner-pending")).hasRevoke).toBe(false);
      // Already revoked: nothing left to invalidate.
      expect((await driver.runnersRowActions("parity-runner-revoked")).hasRevoke).toBe(false);
    });

    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const token = await serverMintApiToken(session, `parity178-revoke-${stamp()}`);
    const created = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, name: `rv178${stamp()}` },
      undefined
    );
    try {
      await test.step("revoking confirms, keeps the row and flips the badge", async () => {
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 45_000 })
          .toContain(created.runnerName);
        // Stall the revoke so the confirmation copy is readable mid-flight.
        await driver.runnersStallMutations(8_000);
        const done = driver.runnersRevokeRunner(created.runnerName);
        const copy = await expect
          .poll(() => driver.runnersModalCopy(), { timeout: 30_000 })
          .not.toBeNull()
          .then(() => driver.runnersModalCopy());
        expect(copy!.title).toBe("Revoke runner?");
        expect(copy!.body).toContain("credentials are invalidated");
        expect(copy!.body).toContain("row stays in the list");
        await done;
        await driver.runnersReleaseMutationShaping();
        const rows = await driver.runnersTableRows();
        expect(rows.find((row) => row.name === created.runnerName)?.status).toBe("revoked");
      });

      await test.step("the server agrees with the screen", async () => {
        expect((await serverRunnerOrNull(created.runnerId, session))?.status).toBe("revoked");
      });
    } finally {
      await driver.runnersReleaseMutationShaping().catch(() => undefined);
      await serverDeleteRunner(created.runnerId, session);
      await serverDeleteApiToken(token.id, session);
    }
  }
);

test(
  specTitle(["RUN-010"], "delete removes the row and toasts on failure"),
  { tag: specTags(["RUN-010"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const token = await serverMintApiToken(session, `parity178-delete-${stamp()}`);
    const victim = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, name: `dl178${stamp()}` },
      undefined
    );
    const survivor = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, name: `ds178${stamp()}` },
      undefined
    );
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the workspace runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([victim.runnerName, survivor.runnerName]));
      });

      await test.step("delete confirms its consequences, then drops the row", async () => {
        await driver.runnersStallMutations(8_000);
        const done = driver.runnersDeleteRunner(victim.runnerName);
        const copy = await expect
          .poll(() => driver.runnersModalCopy(), { timeout: 30_000 })
          .not.toBeNull()
          .then(() => driver.runnersModalCopy());
        expect(copy!.title).toBe("Delete runner?");
        expect(copy!.body).toContain("Historic runs are preserved");
        expect(copy!.body).toContain("does not uninstall");
        await done;
        await driver.runnersReleaseMutationShaping();
        expect(await serverRunnerOrNull(victim.runnerId, session)).toBeNull();
      });

      await test.step("a failed delete toasts and keeps the row and modal", async () => {
        await driver.runnersFailNextMutation();
        await driver.runnersDeleteRunner(survivor.runnerName, { expectFailure: true });
        expect(await driver.runnersLastToast()).toContain("Failed to delete runner");
        expect(await driver.runnersModalCopy()).not.toBeNull();
        await driver.runnersCancelModal();
        await driver.runnersReleaseMutationShaping();
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 30_000 })
          .toContain(survivor.runnerName);
      });
    } finally {
      await driver.runnersReleaseMutationShaping().catch(() => undefined);
      await serverDeleteRunner(survivor.runnerId, session);
      await serverDeleteApiToken(token.id, session);
    }
  }
);

test(
  specTitle(["RUN-010"], "row confirmations block dismissal while in flight"),
  { tag: specTags(["RUN-010"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const token = await serverMintApiToken(session, `parity178-flight-${stamp()}`);
    const doomed = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, name: `df178${stamp()}` },
      undefined
    );
    const revoked = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, name: `rf178${stamp()}` },
      undefined
    );
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the workspace runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([doomed.runnerName, revoked.runnerName]));
      });

      await test.step("the delete confirm ignores cancel mid-flight", async () => {
        await driver.runnersStallMutations(10_000);
        const done = driver.runnersDeleteRunner(doomed.runnerName);
        // The confirm opens before the Delete click lands, so copy alone
        // cannot prove mid-flight: gate the Cancel on the working state,
        // else it may dismiss the modal before the submit fires.
        await driver.runnersWaitRunnerDeleteWorking();
        await expect.poll(() => driver.runnersModalCopy(), { timeout: 30_000 }).not.toBeNull();
        await driver.runnersCancelModal();
        expect(await driver.runnersModalCopy()).not.toBeNull();
        await done;
      });

      await test.step("the revoke confirm ignores cancel mid-flight", async () => {
        const done = driver.runnersRevokeRunner(revoked.runnerName);
        await driver.runnersWaitRunnerRevokeWorking();
        await expect.poll(() => driver.runnersModalCopy(), { timeout: 30_000 }).not.toBeNull();
        await driver.runnersCancelModal();
        expect(await driver.runnersModalCopy()).not.toBeNull();
        await done;
        await driver.runnersReleaseMutationShaping();
      });
    } finally {
      await driver.runnersReleaseMutationShaping().catch(() => undefined);
      await serverDeleteRunner(revoked.runnerId, session);
      await serverDeleteApiToken(token.id, session);
    }
  }
);

test(
  specTitle(["RUN-011"], "each runner status renders its own badge"),
  { tag: specTags(["RUN-011"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      await expect
        .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 60_000 })
        .toEqual(expect.arrayContaining(SEED_RUNNERS));
    });

    await test.step("badges match the four states plus pending-enrolment", async () => {
      // Badge hues are styling; the parity-relevant behavior is the
      // status-to-label mapping (pending enrolment reads as offline).
      const rows = await driver.runnersTableRows();
      const statusOf = (name: string) => rows.find((row) => row.name === name)?.status;
      expect(statusOf("parity-runner-online")).toBe("online");
      expect(statusOf("parity-runner-busy")).toBe("busy");
      expect(statusOf("parity-runner-offline")).toBe("offline");
      expect(statusOf("parity-runner-revoked")).toBe("revoked");
      expect(statusOf("parity-runner-pending")).toBe("offline");
    });

    await test.step("the server agrees with the screen", async () => {
      const session = await signInSession(seed.email, seed.password);
      const workspaceId = await serverWorkspaceId(seed.workspaceSlug, seed.projectId, session);
      const server = await serverRunners(workspaceId, session);
      for (const name of SEED_RUNNERS) {
        const match = server.find((row) => row.name === name);
        expect(match).toBeDefined();
      }
      expect(server.find((row) => row.name === "parity-runner-pending")?.enrolled).toBe(false);
    });
  }
);

test(
  specTitle(["RUN-012"], "pod tiles show name, default marker and runner count"),
  { tag: specTags(["RUN-012"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the workspace runners area", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
    });

    await test.step("tiles match the server pod by pod", async () => {
      const session = await signInSession(seed.email, seed.password);
      const server = await serverPods(seed.projectId, session);
      expect(server.length).toBeGreaterThan(0);
      await expect
        .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
        .toEqual(expect.arrayContaining(server.map((pod) => pod.name)));
      const tiles = await driver.runnersPodTiles();
      expect(new Set(tiles.map((tile) => tile.name))).toEqual(new Set(server.map((pod) => pod.name)));
      for (const tile of tiles) {
        const match = server.find((pod) => pod.name === tile.name);
        expect(match).toBeDefined();
        expect(tile.isDefault).toBe(match!.isDefault);
        expect(tile.runnerCount).toBe(`${match!.runnerCount} runner(s)`);
      }
    });
  }
);

test(
  specTitle(["RUN-012"], "clicking a tile filters the table with a clear affordance"),
  { tag: specTags(["RUN-012"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const suffix = `f178${stamp()}`;
    const pod = await createPod(seed.projectId, session, suffix);
    const token = await serverMintApiToken(session, `parity178-filter-${stamp()}`);
    const member = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, pod: pod.name, name: `fb178${stamp()}` },
      undefined
    );
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the workspace runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
          .toContain(pod.name);
      });

      await test.step("the filter narrows rows to the pod and clears back", async () => {
        await driver.runnersSelectPod(pod.name);
        await expect.poll(() => driver.runnersFilterText(), { timeout: 30_000 }).toContain(pod.name);
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 30_000 })
          .toEqual([member.runnerName]);
        await driver.runnersClearPodFilter();
        expect(await driver.runnersFilterText()).toBeNull();
        await expect
          .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 30_000 })
          .toEqual(expect.arrayContaining(SEED_RUNNERS));
      });

      await test.step("filtering fires no list fetch", async () => {
        expect(await driver.runnersPodFilterIsClientSide(pod.name)).toBe(true);
        expect(await driver.runnersFilterText()).toBeNull();
      });
    } finally {
      await serverDeleteRunner(member.runnerId, session);
      await deletePod(pod.id, session);
      await serverDeleteApiToken(token.id, session);
    }
  }
);

test(
  specTitle(["RUN-012"], "deleting the filtered pod clears the filter"),
  { tag: specTags(["RUN-012"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const pod = await createPod(seed.projectId, session, `h178${stamp()}`);

    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("filter on the empty pod", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      await expect
        .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
        .toContain(pod.name);
      await driver.runnersSelectPod(pod.name);
      await expect.poll(() => driver.runnersFilterText(), { timeout: 30_000 }).toContain(pod.name);
      await expect.poll(() => driver.runnersTableRows(), { timeout: 30_000 }).toEqual([]);
    });

    await test.step("deleting it self-heals the list", async () => {
      await driver.runnersDeletePod(pod.name);
      expect(await driver.runnersFilterText()).toBeNull();
      await expect
        .poll(() => driver.runnersTableRows().then((rows) => rows.map((row) => row.name)), { timeout: 30_000 })
        .toEqual(expect.arrayContaining(SEED_RUNNERS));
      expect((await serverPods(seed.projectId, session)).map((entry) => entry.name)).not.toContain(pod.name);
    });
  }
);

test(
  specTitle(["RUN-012"], "a pods outage renders the load-failure message"),
  { tag: specTags(["RUN-012"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the area with pod reads failing", async () => {
      await driver.runnersFailPodsLoad();
      await driver.runnersOpenOverview(seed.workspaceSlug);
    });

    await test.step("the failure message shows instead of tiles", async () => {
      await expect.poll(() => driver.runnersPodsError(), { timeout: 30_000 }).toContain("Failed to load pods");
      expect(await driver.runnersPodTiles()).toEqual([]);
      await driver.runnersReleasePodsFailure();
    });
  }
);

test(
  specTitle(["RUN-013"], "create-pod validates, prefixes, submits and resets"),
  { tag: specTags(["RUN-013"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const suffix = `c178${stamp()}`;
    const fullName = `${identifier}_${suffix}`;

    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("open the create-pod modal", async () => {
      await driver.runnersOpenOverview(seed.workspaceSlug);
      await driver.runnersOpenCreatePod();
    });

    await test.step("empty submit names both required fields", async () => {
      await driver.runnersSubmitPodForm();
      await expect
        .poll(() => driver.runnersPodFormErrors(), { timeout: 30_000 })
        .toEqual(expect.arrayContaining(["Pick a project.", "Name is required."]));
    });

    await test.step("submit shows a working state with cancel disabled", async () => {
      await driver.runnersStallMutations(8_000);
      const done = driver.runnersCreatePod(
        { projectName: seed.projectName, name: suffix, description: "178 pod desc" },
        undefined
      );
      await expect.poll(() => driver.runnersCreatePodSubmitting(), { timeout: 30_000 }).toBe(true);
      expect(await driver.runnersPodCancelEnabled()).toBe(false);
      await done;
      await driver.runnersReleaseMutationShaping();
    });

    await test.step("the prefixed tile lands and the server agrees", async () => {
      await expect
        .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 30_000 })
        .toContain(fullName);
      const match = (await serverPods(seed.projectId, session)).find((pod) => pod.name === fullName);
      expect(match).toBeDefined();
      expect(match!.isDefault).toBe(false);
      expect(match!.description).toBe("178 pod desc");
    });

    await test.step("reopening resets the form to defaults", async () => {
      await driver.runnersOpenCreatePod();
      expect(await driver.runnersCreatePodForm()).toEqual({ project: "Select a project", name: "", description: "" });
      await driver.runnersCancelModal();
      await expect.poll(() => driver.runnersPodModalOpen(), { timeout: 30_000 }).toBe(false);
    });

    await test.step("a failed create toasts and keeps the modal", async () => {
      const doomed = (await serverPods(seed.projectId, session)).find((pod) => pod.name === fullName)!;
      await driver.runnersOpenCreatePod();
      await driver.runnersFailNextMutation();
      await driver.runnersCreatePod({ projectName: seed.projectName, name: `x178${stamp()}` }, { expectFailure: true });
      expect(await driver.runnersLastToast()).toContain("Could not create the pod.");
      expect(await driver.runnersPodModalOpen()).toBe(true);
      await driver.runnersCancelModal();
      await driver.runnersReleaseMutationShaping();
      await deletePod(doomed.id, session);
    });
  }
);

test(
  specTitle(["RUN-014"], "edit-pod renames the suffix and sends only changes"),
  { tag: specTags(["RUN-014"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const suffixD = `d178${stamp()}`;
    const suffixE = `e178${stamp()}`;
    const podD = await createPod(seed.projectId, session, suffixD);
    const podE = await createPod(seed.projectId, session, suffixE);
    const renamed = `d178r${stamp()}`;
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the workspace runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([podD.name, podE.name]));
      });

      await test.step("the form edits the suffix and re-seeds per pod", async () => {
        await driver.runnersOpenEditPod(podD.name);
        expect(await driver.runnersEditPodForm()).toEqual({
          name: suffixD,
          description: "",
          isDefault: false,
          defaultDisabled: false,
        });
        await driver.runnersCancelModal();
        await expect.poll(() => driver.runnersPodModalOpen(), { timeout: 30_000 }).toBe(false);
        await driver.runnersOpenEditPod(podE.name);
        expect((await driver.runnersEditPodForm()).name).toBe(suffixE);
        await driver.runnersCancelModal();
        await expect.poll(() => driver.runnersPodModalOpen(), { timeout: 30_000 }).toBe(false);
      });

      await test.step("rename-only sends only the name", async () => {
        await driver.runnersOpenEditPod(podD.name);
        const bodies = await driver.runnersSavePodEditCapturing({ name: renamed });
        expect(bodies).toEqual([{ name: renamed }]);
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 30_000 })
          .toContain(`${identifier}_${renamed}`);
      });

      await test.step("description-only sends only the description", async () => {
        await driver.runnersOpenEditPod(`${identifier}_${renamed}`);
        const bodies = await driver.runnersSavePodEditCapturing({ description: "renamed 178" });
        expect(bodies).toEqual([{ description: "renamed 178" }]);
        const match = (await serverPods(seed.projectId, session)).find((pod) => pod.id === podD.id);
        expect(match?.description).toBe("renamed 178");
      });

      await test.step("an unchanged save fires no update", async () => {
        await driver.runnersOpenEditPod(`${identifier}_${renamed}`);
        expect(await driver.runnersUnchangedEditSkipsSave()).toBe(true);
      });
    } finally {
      await deletePod(podD.id, session);
      await deletePod(podE.id, session);
    }
  }
);

test(
  specTitle(["RUN-014"], "promoting a pod transfers the project default"),
  { tag: specTags(["RUN-014"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const pods = await serverPods(seed.projectId, session);
    const current = pods.find((pod) => pod.isDefault)!;
    const podF = await createPod(seed.projectId, session, `f178${stamp()}`);
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("the current default cannot un-default itself", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
          .toContain(podF.name);
        await driver.runnersOpenEditPod(current.name);
        const form = await driver.runnersEditPodForm();
        expect(form.isDefault).toBe(true);
        expect(form.defaultDisabled).toBe(true);
        await driver.runnersCancelModal();
        await expect.poll(() => driver.runnersPodModalOpen(), { timeout: 30_000 }).toBe(false);
      });

      await test.step("promoting the newcomer moves the marker and the flag", async () => {
        await driver.runnersOpenEditPod(podF.name);
        expect((await driver.runnersEditPodForm()).defaultDisabled).toBe(false);
        const bodies = await driver.runnersSavePodEditCapturing({ makeDefault: true });
        expect(bodies).toEqual([{ is_default: true }]);
        await expect
          .poll(() => driver.runnersPodTiles(), { timeout: 30_000 })
          .toEqual(
            expect.arrayContaining([
              expect.objectContaining({ name: podF.name, isDefault: true }),
              expect.objectContaining({ name: current.name, isDefault: false }),
            ])
          );
        const after = await serverPods(seed.projectId, session);
        expect(after.find((pod) => pod.id === podF.id)?.isDefault).toBe(true);
        expect(after.find((pod) => pod.id === current.id)?.isDefault).toBe(false);
      });
    } finally {
      await serverPatchPod(current.id, session, { is_default: true });
      await deletePod(podF.id, session);
    }
  }
);

test(
  specTitle(["RUN-014"], "the default pod is where issue delegation lands"),
  { tag: specTags(["RUN-014"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const workspaceId = await serverWorkspaceId(seed.workspaceSlug, seed.projectId, session);
    const pods = await serverPods(seed.projectId, session);
    const current = pods.find((pod) => pod.isDefault)!;
    const podG = await createPod(seed.projectId, session, `g178${stamp()}`);
    const issueX = await createIssue(seed.workspaceSlug, seed.projectId, session, `parity178-deleg-x-${stamp()}`);
    const issueY = await createIssue(seed.workspaceSlug, seed.projectId, session, `parity178-deleg-y-${stamp()}`);
    await serverUnpinIssuePod(seed.workspaceSlug, seed.projectId, issueX.id, session);
    await serverUnpinIssuePod(seed.workspaceSlug, seed.projectId, issueY.id, session);
    expect(await serverIssuePodPin(seed.workspaceSlug, seed.projectId, issueX.id, session)).toBeNull();
    expect(await serverIssuePodPin(seed.workspaceSlug, seed.projectId, issueY.id, session)).toBeNull();
    const runIds: string[] = [];
    try {
      await test.step("an unpinned issue delegates to the current default", async () => {
        const run = await createRunOrFind(session, {
          workspaceId,
          prompt: `parity178 delegation pre ${stamp()}`,
          workItemId: issueX.id,
        });
        runIds.push(run.id);
        expect(run.pod).toBe(current.id);
        await serverCancelRun(run.id, session, "parity178 cleanup");
      });

      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("promote the newcomer through the UI", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
          .toContain(podG.name);
        await driver.runnersOpenEditPod(podG.name);
        await driver.runnersSavePodEdit({ makeDefault: true });
        await expect
          .poll(() => driver.runnersPodTiles(), { timeout: 30_000 })
          .toEqual(expect.arrayContaining([expect.objectContaining({ name: podG.name, isDefault: true })]));
      });

      await test.step("delegation follows the new default", async () => {
        const run = await createRunOrFind(session, {
          workspaceId,
          prompt: `parity178 delegation post ${stamp()}`,
          workItemId: issueY.id,
        });
        runIds.push(run.id);
        expect(run.pod).toBe(podG.id);
        await serverCancelRun(run.id, session, "parity178 cleanup");
      });
    } finally {
      for (const runId of runIds) {
        await serverCancelRun(runId, session, "parity178 cleanup").catch(() => undefined);
      }
      await deleteIssue(seed.workspaceSlug, seed.projectId, issueX.id, session).catch(() => undefined);
      await deleteIssue(seed.workspaceSlug, seed.projectId, issueY.id, session).catch(() => undefined);
      await serverPatchPod(current.id, session, { is_default: true });
      await deletePod(podG.id, session);
    }
  }
);

test(
  specTitle(["RUN-015"], "delete-pod confirms its consequences and blocks mid-flight dismissal"),
  { tag: specTags(["RUN-015"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const pod = await createPod(seed.projectId, session, `k178${stamp()}`);
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the workspace runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
          .toContain(pod.name);
      });

      await test.step("the confirm warns, ignores cancel mid-flight, then drops the tile", async () => {
        await driver.runnersStallMutations(8_000);
        const done = driver.runnersDeletePod(pod.name);
        // The confirm opens before the Delete click lands, so copy alone
        // cannot prove mid-flight: gate the Cancel on the working state.
        await driver.runnersWaitPodDeleteWorking();
        const copy = await expect
          .poll(() => driver.runnersModalCopy(), { timeout: 30_000 })
          .not.toBeNull()
          .then(() => driver.runnersModalCopy());
        expect(copy!.title).toBe("Delete pod?");
        expect(copy!.body).toContain("unassigned");
        expect(copy!.body).toContain("preserved");
        await driver.runnersCancelModal();
        expect(await driver.runnersModalCopy()).not.toBeNull();
        await done;
        await driver.runnersReleaseMutationShaping();
        expect((await serverPods(seed.projectId, session)).map((entry) => entry.name)).not.toContain(pod.name);
      });
    } finally {
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-015"], "a pod with runners refuses deletion with its guard message"),
  { tag: specTags(["RUN-015"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
    const pod = await createPod(seed.projectId, session, `j178${stamp()}`);
    const token = await serverMintApiToken(session, `parity178-podguard-${stamp()}`);
    const member = await serverCreateRunnerV1(
      token.token,
      { project: identifier, workspaceSlug: seed.workspaceSlug, pod: pod.name, name: `jg178${stamp()}` },
      undefined
    );
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the workspace runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
          .toContain(pod.name);
      });

      await test.step("the UI translates the guard and keeps the tile", async () => {
        await driver.runnersDeletePod(pod.name, { expectFailure: true });
        expect(await driver.runnersLastToast()).toContain("pod's runners");
        expect(await driver.runnersModalCopy()).not.toBeNull();
        await driver.runnersCancelModal();
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 30_000 })
          .toContain(pod.name);
      });

      await test.step("the server reports the guard code", async () => {
        const result = await serverDeletePodResult(pod.id, session);
        expect(result.status).toBe(409);
        expect(result.code).toBe("pod_has_runners");
      });
    } finally {
      await serverDeleteRunner(member.runnerId, session);
      await deletePod(pod.id, session);
      await serverDeleteApiToken(token.id, session);
    }
  }
);

test(
  specTitle(["RUN-015"], "the default pod refuses deletion until another is promoted"),
  { tag: specTags(["RUN-015"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const pods = await serverPods(seed.projectId, session);
    const current = pods.find((pod) => pod.isDefault)!;
    const pod = await createPod(seed.projectId, session, `m178${stamp()}`);
    await serverPatchPod(pod.id, session, { is_default: true });
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the workspace runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersPodTiles(), { timeout: 60_000 })
          .toEqual(expect.arrayContaining([expect.objectContaining({ name: pod.name, isDefault: true })]));
      });

      await test.step("the UI translates the guard and keeps the tile", async () => {
        await driver.runnersDeletePod(pod.name, { expectFailure: true });
        expect(await driver.runnersLastToast()).toContain("default pod");
        expect(await driver.runnersModalCopy()).not.toBeNull();
        await driver.runnersCancelModal();
      });

      await test.step("the server reports the guard code", async () => {
        const result = await serverDeletePodResult(pod.id, session);
        expect(result.status).toBe(409);
        expect(result.code).toBe("default_pod_undeletable");
      });
    } finally {
      await serverPatchPod(current.id, session, { is_default: true });
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-015"], "a pod with active runs refuses deletion with its guard message"),
  { tag: specTags(["RUN-015"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const workspaceId = await serverWorkspaceId(seed.workspaceSlug, seed.projectId, session);
    const pod = await createPod(seed.projectId, session, `n178${stamp()}`);
    const issue = await createIssue(seed.workspaceSlug, seed.projectId, session, `parity178-guard-${stamp()}`);
    const run = await createRunOrFind(session, {
      workspaceId,
      prompt: `parity178 guard run ${stamp()}`,
      workItemId: issue.id,
      podId: pod.id,
    });
    expect(run.pod).toBe(pod.id);
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });

      await test.step("open the workspace runners area", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug);
        await expect
          .poll(() => driver.runnersPodTiles().then((tiles) => tiles.map((tile) => tile.name)), { timeout: 60_000 })
          .toContain(pod.name);
      });

      await test.step("the UI translates the guard and keeps the tile", async () => {
        await driver.runnersDeletePod(pod.name, { expectFailure: true });
        expect(await driver.runnersLastToast()).toContain("active runs");
        expect(await driver.runnersModalCopy()).not.toBeNull();
        await driver.runnersCancelModal();
      });

      await test.step("the server reports the guard code", async () => {
        const result = await serverDeletePodResult(pod.id, session);
        expect(result.status).toBe(409);
        expect(result.code).toBe("pod_has_active_runs");
      });
    } finally {
      await serverCancelRun(run.id, session, "parity178 cleanup").catch(() => undefined);
      await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session).catch(() => undefined);
      await deletePod(pod.id, session);
    }
  }
);
