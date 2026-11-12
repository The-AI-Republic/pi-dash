// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWPIDASH-3): the runners rail fills its column. Row:
// RUN-004. The rail and the content pane beside it reach the bottom of
// the page frame at common heights, in the workspace and project scopes,
// with no runners connected and with more runners than fit on screen. In
// the new app the frame ends at the window's bottom edge.
import { test, expect } from "../fixtures";
import {
  getWorkspaceUserProperties,
  parityProjectIdentifier,
  patchWorkspaceUserProperties,
  projectFacts,
  serverCreateProjectWithFlags,
  serverCreateRunnerV1,
  serverDeleteApiToken,
  serverDeleteProject,
  serverDeleteRunner,
  serverMintApiToken,
  signInSession,
} from "../helpers/api";
import type { ParityDriver } from "../drivers/index";
import { specTags, specTitle } from "../helpers/tags";

const stamp = () => Date.now().toString(36);

/** Common window sizes: a small laptop and a full-HD desktop. */
const VIEWPORTS = [
  { width: 1280, height: 720 },
  { width: 1920, height: 1080 },
] as const;

/** Enough runners to overflow the rail at the taller window. */
const MANY_RUNNERS = 24;

/** Sub-pixel slack for layout rounding. */
const TOLERANCE_PX = 1;

/**
 * Gap between the page frame's bottom and the window's. The old app draws
 * every page inside an inset card (8px padding plus a 1px border), so its
 * rail fills the card; the new app's content area runs to the window edge.
 */
function frameInsetPx(driver: ParityDriver): number {
  return driver.target === "web_new" ? 0 : 9;
}

async function expectRailFillsWindow(
  driver: ParityDriver,
  open: () => Promise<void>,
  scopeLabel: string
): Promise<void> {
  for (const viewport of VIEWPORTS) {
    await test.step(`${scopeLabel} at ${viewport.width}x${viewport.height}`, async () => {
      await driver.setViewportSize(viewport.width, viewport.height);
      await open();
      await expect
        .poll(
          async () => {
            const geometry = await driver.runnersRailGeometry();
            const frameBottom = geometry.viewportHeight - frameInsetPx(driver);
            return {
              rail: Math.abs(geometry.railBottom - frameBottom) <= TOLERANCE_PX,
              content: Math.abs(geometry.contentBottom - frameBottom) <= TOLERANCE_PX,
            };
          },
          { timeout: 30_000 }
        )
        .toEqual({ rail: true, content: true });
      const geometry = await driver.runnersRailGeometry();
      expect(geometry.railTop).toBeLessThan(geometry.viewportHeight / 2);
    });
  }
}

test(
  specTitle(["RUN-004"], "the rail and the content pane reach the bottom of the window in the workspace scope"),
  { tag: specTags(["RUN-004"]) },
  async ({ driver, seed }) => {
    await test.step("sign in as the owner", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });
    await expectRailFillsWindow(driver, () => driver.runnersOpenOverview(seed.workspaceSlug), "workspace scope");
  }
);

test(
  specTitle(["RUN-004"], "the rail reaches the bottom of the window in a project with no runners"),
  { tag: specTags(["RUN-004"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const emptyId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `runners raillayout empty ${stamp()}`,
      parityProjectIdentifier("RLE"),
      {},
      session
    );
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });
      await test.step("the project rail shows its empty state", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug, emptyId);
        await expect.poll(() => driver.runnersRail().then((rail) => rail.emptyVisible)).toBe(true);
      });
      await expectRailFillsWindow(
        driver,
        () => driver.runnersOpenOverview(seed.workspaceSlug, emptyId),
        "empty project scope"
      );
    } finally {
      await serverDeleteProject(seed.workspaceSlug, emptyId, session);
    }
  }
);

test(
  specTitle(["RUN-004"], "the rail stays window height in a project with more runners than fit"),
  { tag: specTags(["RUN-004"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `runners raillayout many ${stamp()}`,
      parityProjectIdentifier("RLM"),
      {},
      session
    );
    const { identifier } = await projectFacts(seed.workspaceSlug, projectId, session);
    const token = await serverMintApiToken(session, `parity-np3-rail-${stamp()}`);
    const runnerIds: string[] = [];
    try {
      for (let index = 0; index < MANY_RUNNERS; index += 1) {
        const created = await serverCreateRunnerV1(token.token, {
          project: identifier,
          workspaceSlug: seed.workspaceSlug,
          name: `rl${index}${stamp()}`,
        });
        runnerIds.push(created.runnerId);
      }
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });
      await test.step("the project rail lists every runner", async () => {
        await driver.runnersOpenOverview(seed.workspaceSlug, projectId);
        await expect
          .poll(() => driver.runnersRail().then((rail) => rail.contacts.length), { timeout: 60_000 })
          .toBe(MANY_RUNNERS);
      });
      await expectRailFillsWindow(
        driver,
        () => driver.runnersOpenOverview(seed.workspaceSlug, projectId),
        "crowded project scope"
      );
    } finally {
      for (const runnerId of runnerIds) await serverDeleteRunner(runnerId, session);
      await serverDeleteApiToken(token.id, session);
      await serverDeleteProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["RUN-004"], "the rail reaches the bottom of the window below the project tab strip"),
  { tag: specTags(["RUN-004"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const modeBefore = (await getWorkspaceUserProperties(seed.workspaceSlug, session))[
      "navigation_control_preference"
    ] as string | undefined;
    await patchWorkspaceUserProperties(seed.workspaceSlug, session, { navigation_control_preference: "TABBED" });
    try {
      await test.step("sign in as the owner", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
      });
      await expectRailFillsWindow(
        driver,
        () => driver.runnersOpenOverview(seed.workspaceSlug, seed.projectId),
        "tabbed project scope"
      );
    } finally {
      await patchWorkspaceUserProperties(seed.workspaceSlug, session, {
        navigation_control_preference: modeBefore ?? "ACCORDION",
      });
    }
  }
);
