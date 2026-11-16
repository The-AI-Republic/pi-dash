// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-444): instance-admin AI settings and the
// Image page absence. Rows ADM-046 (OpenAI model plus API key),
// ADM-047 (other-vendor callout), ADM-048 (AI loading skeleton),
// ADM-049 (Image page deleted — absence) and ADM-050 (no image
// skeleton — absence).
//
// Observed behavior notes (paraphrased; the inventory rows carry them at
// update time): the AI form carries an optional model text field and an
// optional password key field under an OpenAI heading with helper links,
// and its save stays enabled even when both are empty; success toasts,
// while a failed save lands in the console only with no notice. A static
// callout invites admins preferring another vendor to make contact. A
// header/grid/button skeleton shows until configurations load. The Image
// page is still served on this checkout (bug: NEWFRONT-493): the sidebar
// keeps its Images entry, the route renders the Unsplash-key form with a
// save action, and a skeleton shows while loading. Intended per
// main#523: no Image page, route, sidebar entry, Unsplash key, or
// skeleton anywhere.
import { test, expect } from "../fixtures";
import { serverAdminGetConfigurations, serverAdminSession, serverAdminUpdateConfigurations } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Configuration keys this spec snapshots and restores. */
const AI_KEYS = ["LLM_MODEL", "LLM_API_KEY"] as const;

/** Local sleep for proving an absence (no write, no notice) over a window. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Snapshot the AI rows this spec may rewrite, for teardown restore. */
async function snapshotAi(session: string): Promise<Record<string, string>> {
  const all = await serverAdminGetConfigurations(session);
  return Object.fromEntries(AI_KEYS.map((key) => [key, all[key] ?? ""]));
}

/** Restore AI rows and prove the restore landed (keeps retries convergent). */
async function restoreAi(session: string, snap: Record<string, string>): Promise<void> {
  await serverAdminUpdateConfigurations(session, snap);
  const after = await serverAdminGetConfigurations(session);
  for (const key of AI_KEYS) {
    expect(after[key]).toBe(snap[key]);
  }
}

/** Open the AI page signed in, waiting for the form to render. */
async function openAi(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath("/ai/");
  await driver.adminWaitForRootBranch("dashboard");
  await expect.poll(async () => (await driver.adminAiFacts()).formVisible, { timeout: 15_000 }).toBe(true);
}

test(
  specTitle(["ADM-046"], "model and key persist with a success notice"),
  { tag: specTags(["ADM-046"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAi(session);
    try {
      await openAi(driver, seed);
      await driver.adminStartConfigPatchSpy();
      const model = `gpt-probe-${Date.now().toString(36)}`;
      const key = `sk-probe-${Date.now().toString(36)}`;
      await driver.adminAiFill(model, key);
      await driver.adminAiSave();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Success");
      const toast = await driver.adminLastToast();
      expect(toast?.message).toContain("AI Settings updated successfully");
      const bodies = await driver.adminConfigPatchBodies();
      expect(bodies.length).toBeGreaterThan(0);
      const after = await serverAdminGetConfigurations(session);
      expect(after["LLM_MODEL"]).toBe(model);
      expect(after["LLM_API_KEY"]).toBe(key);
      const facts = await driver.adminAiFacts();
      expect(facts.model).toBe(model);
      expect(facts.keyFilled).toBe(true);
    } finally {
      await restoreAi(session, before);
    }
  }
);

test(
  specTitle(["ADM-046"], "empty save is allowed and the save stays enabled"),
  { tag: specTags(["ADM-046"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAi(session);
    try {
      await openAi(driver, seed);
      await driver.adminAiFill("", "");
      const facts = await driver.adminAiFacts();
      expect(facts.saveDisabled).toBe(false);
      await driver.adminAiSave();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Success");
      const after = await serverAdminGetConfigurations(session);
      expect(after["LLM_MODEL"]).toBe("");
      expect(after["LLM_API_KEY"]).toBe("");
    } finally {
      await restoreAi(session, before);
    }
  }
);

test(
  specTitle(["ADM-046"], "failed save logs to the console with no notice"),
  { tag: specTags(["ADM-046"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAi(session);
    try {
      await driver.adminStartConsoleSpy();
      await openAi(driver, seed);
      await driver.adminStartConfigPatchSpy();
      await driver.adminAiFill("gpt-fail-probe", "sk-fail-probe");
      await driver.adminFailConfigPatch();
      await driver.adminAiSave();
      await sleep(2000);
      expect(await driver.adminLastToast()).toBe(null);
      const errors = await driver.adminConsoleErrors();
      expect(errors.length).toBeGreaterThan(0);
      const after = await serverAdminGetConfigurations(session);
      expect(after["LLM_MODEL"]).toBe(before["LLM_MODEL"]);
      expect(after["LLM_API_KEY"]).toBe(before["LLM_API_KEY"]);
    } finally {
      await driver.adminClearConfigPatchFault().catch(() => {});
      await restoreAi(session, before);
    }
  }
);

test(
  specTitle(["ADM-046"], "key show/hide toggle flips the field type"),
  { tag: specTags(["ADM-046"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAi(session);
    try {
      await openAi(driver, seed);
      await driver.adminAiFill("gpt-toggle-probe", "sk-toggle-probe");
      let facts = await driver.adminAiFacts();
      expect(facts.keyShown).toBe(false);
      await driver.adminAiToggleKeyVisibility();
      facts = await driver.adminAiFacts();
      expect(facts.keyShown).toBe(true);
      await driver.adminAiToggleKeyVisibility();
      facts = await driver.adminAiFacts();
      expect(facts.keyShown).toBe(false);
      expect(facts.keyFilled).toBe(true);
    } finally {
      await restoreAi(session, before);
    }
  }
);

test(
  specTitle(["ADM-046"], "helper links point at the OpenAI docs"),
  { tag: specTags(["ADM-046"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openAi(driver, seed);
    const facts = await driver.adminAiFacts();
    expect(facts.modelHelpHref).toContain("platform.openai.com/docs/models");
    expect(facts.keyHelpHref).toContain("platform.openai.com/api-keys");
    const after = await serverAdminGetConfigurations(await serverAdminSession(seed.email, seed.password));
    expect(typeof after["LLM_MODEL"]).toBe("string");
  }
);

test(
  specTitle(["ADM-047"], "other-vendor callout carries the contact link"),
  { tag: specTags(["ADM-047"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openAi(driver, seed);
    const facts = await driver.adminAiFacts();
    expect(facts.calloutVisible).toBe(true);
    expect(facts.calloutHref).toContain("airepublic.com/contact");
  }
);

test(
  specTitle(["ADM-048"], "skeleton shows until configurations load"),
  { tag: specTags(["ADM-048"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    // The read is held until the fault is cleared, so a cold route
    // compile on the new console cannot outlast the skeleton window.
    await driver.adminDelayConfigFetch(driver.target === "web_new" ? 120_000 : 3000);
    try {
      await driver.adminOpenPath("/ai/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminAiFacts()).skeletonVisible, { timeout: 15_000 }).toBe(true);
      const during = await driver.adminAiFacts();
      expect(during.formVisible).toBe(false);
      expect(during.skeletonBlocks).toBeGreaterThan(0);
    } finally {
      await driver.adminClearConfigFetchFault();
    }
    await expect.poll(async () => (await driver.adminAiFacts()).formVisible, { timeout: 90_000 }).toBe(true);
    const after = await driver.adminAiFacts();
    expect(after.skeletonVisible).toBe(false);
  }
);

test(
  specTitle(["ADM-049"], "bug: NEWFRONT-493 image page is still served with its sidebar entry"),
  { tag: specTags(["ADM-049"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    await driver.adminOpenPath("/workspace/");
    await driver.adminWaitForRootBranch("dashboard");
    let facts = await driver.adminImageFacts();
    if (driver.target === "web_new") {
      // Intended (main#523): the image area is gone — no rail entry and
      // no page behind its address, which falls to the catch-all.
      expect(facts.sidebarEntryVisible).toBe(false);
      expect(facts.sidebarHref).toBeNull();
      await driver.adminOpenPath("/image/");
      const notFound = await driver.adminNotFoundFacts();
      expect(notFound.illustrationVisible).toBe(true);
      expect(notFound.messageVisible).toBe(true);
      expect(await driver.adminCurrentPath()).toBe("/god-mode/image/");
      expect(await driver.adminDashboardVisible()).toBe(false);
      facts = await driver.adminImageFacts();
      expect(facts.pageTitle).toBeNull();
      expect(facts.keyFieldVisible).toBe(false);
      expect(facts.saveVisible).toBe(false);
      expect(facts.skeletonVisible).toBe(false);
      return;
    }
    expect(facts.sidebarEntryVisible).toBe(true);
    expect(facts.sidebarHref).toContain("/image/");
    await driver.adminOpenPath("/image/");
    await driver.adminWaitForRootBranch("dashboard");
    facts = await driver.adminImageFacts();
    expect(facts.pageTitle).toContain("Third-party image libraries");
    expect(facts.keyFieldVisible).toBe(true);
    expect(facts.saveVisible).toBe(true);
  }
);

test(
  specTitle(["ADM-050"], "bug: NEWFRONT-493 image skeleton shows while loading"),
  { tag: specTags(["ADM-050"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    await driver.adminDelayConfigFetch(3000);
    if (driver.target === "web_new") {
      // No image page means no image skeleton, even under a stalled
      // configurations read: the address stays on the catch-all.
      await driver.adminOpenPath("/image/");
      expect((await driver.adminNotFoundFacts()).messageVisible).toBe(true);
      const during = await driver.adminImageFacts();
      expect(during.skeletonVisible).toBe(false);
      expect(during.keyFieldVisible).toBe(false);
      await driver.adminClearConfigFetchFault();
      await driver.adminOpenPath("/ai/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminAiFacts()).formVisible, { timeout: 15_000 }).toBe(true);
      return;
    }
    await driver.adminOpenPath("/image/");
    await driver.adminWaitForRootBranch("dashboard");
    await expect.poll(async () => (await driver.adminImageFacts()).skeletonVisible, { timeout: 15_000 }).toBe(true);
    const during = await driver.adminImageFacts();
    expect(during.keyFieldVisible).toBe(false);
    await driver.adminClearConfigFetchFault();
    await expect.poll(async () => (await driver.adminImageFacts()).keyFieldVisible, { timeout: 15_000 }).toBe(true);
    const after = await driver.adminImageFacts();
    expect(after.skeletonVisible).toBe(false);
  }
);
