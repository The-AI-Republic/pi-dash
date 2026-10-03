// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-125): the workspace logo shows the uploaded
// image or the workspace initial with accessible labeling.
// Observed on the running old app: the seeded workspace has no logo
// uploaded, so the top-bar switcher button shows the workspace initial
// under an accessible switcher label; setting a logo renders the image,
// and clearing it falls back to the initial. Row: SHELL-061.
import { test, expect } from "../../fixtures";
import { deleteWorkspace, ensureWorkspace, ownerSession, patchWorkspaceLogo } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-061"];
// A self-contained mark: no network fetch, always renders as an image.
const LOGO_DATA_URL =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16'%3E%3Crect width='16' height='16' fill='%23000'/%3E%3C/svg%3E";

test(
  specTitle(ROWS, "workspace logo renders uploads and falls back to the initial"),
  {
    tag: specTags(ROWS),
  },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("the mark shows the initial with accessible labeling", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      const logo = await driver.workspaceLogoState();
      expect(logo.hasImage).toBe(false);
      expect(logo.initial).toBe(seed.workspaceName.slice(0, 1));
      expect(logo.label ?? "").toMatch(/workspace/i);
    });

    await test.step("an uploaded logo renders the image", async () => {
      // A dedicated workspace: the seed workspace must keep its logoless
      // baseline for the fallback step above and for sibling runs.
      const branded = await ensureWorkspace(session, "Parity Brand", "parity-brand");
      try {
        await patchWorkspaceLogo(branded.slug, session, LOGO_DATA_URL);
        await driver.openWorkspacePath(`/${branded.slug}/`);
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
        const withLogo = await driver.workspaceLogoState();
        expect(withLogo.hasImage).toBe(true);
        expect(withLogo.label ?? "").toMatch(/logo/i);

        await patchWorkspaceLogo(branded.slug, session, null);
        await driver.openWorkspacePath(`/${branded.slug}/`);
        await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
        const cleared = await driver.workspaceLogoState();
        expect(cleared.hasImage).toBe(false);
        expect(cleared.initial).toBe("P");
      } finally {
        await deleteWorkspace(session, branded.slug);
      }
    });
  }
);
