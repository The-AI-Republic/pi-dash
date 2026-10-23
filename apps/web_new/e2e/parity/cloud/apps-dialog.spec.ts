// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): key-based connects, row CLOUD-027
// (credential dialog built from the server manifest; inline blocking;
// secrets cleared after submit; never in URLs), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): a manual app's Connect opens
// a generic dialog whose fields, labels, placeholders, masking, and
// format checks come from the server manifest (a bare api_key type
// falls back to a single key field); publisher instructions render as
// plain text with emphasis markers dropped and markup uninterpretted,
// and the issuance link renders only for safely absolute HTTPS URLs;
// empty or malformed values block submit inline with every problem
// surfaced at once and no request sent; submit posts trimmed values,
// clears them, toasts, closes, and re-reads the catalog; a rejected
// credential toasts and leaves the dialog open. Live, the server
// rejects empty field posts before any gateway call.
import { test, expect } from "../fixtures";
import { cloudApiKeyRaw, cloudBrowserCookies, cloudDevLogin } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { installOpenhubStub, marketplacePage, stubApp } from "./openhub-stub";

const MANUAL_APP = stubApp({
  appId: "app-manual",
  slug: "manu",
  name: "Manual CRM",
  authType: "api_key",
  isInstalled: true,
  suggestedAction: "connect_auth",
});

const MANIFEST = {
  setupUrl: "https://crm.example.com/keys",
  instructions_md: "Paste your **API key** below.\n<script>alert(1)</script>",
  fields: [
    { key: "api_key", label: "API key", type: "secret", placeholder: "sk-...", validation: "^sk-[a-z0-9]{4,}$" },
    { key: "region", label: "Region", type: "text", placeholder: "us" },
  ],
};

test(
  specTitle(["CLOUD-027"], "dialog builds its fields from the server manifest"),
  { tag: specTags(["CLOUD-027"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([MANUAL_APP]) }),
      authStatus: () =>
        Promise.resolve({ status: 200, body: { type: "api_key", status: "needs_auth", manualSetup: MANIFEST } }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Manual CRM"]);
      await driver.cloudAppsClickCardAction("Manual CRM", "Connect");
      await expect.poll(() => driver.cloudAppsDialogVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.cloudAppsDialogTitle()).toContain("Manual CRM");
      expect(await driver.cloudAppsDialogFields()).toEqual([
        { label: "API key", inputType: "password", placeholder: "sk-..." },
        { label: "Region", inputType: "text", placeholder: "us" },
      ]);
      const instructions = (await driver.cloudAppsDialogInstructions()) ?? "";
      expect(instructions).toContain("Paste your API key below.");
      expect(instructions).not.toContain("**");
      expect(instructions).toContain("<script>alert(1)</script>");
      expect(await driver.page.locator("div.fixed.inset-0 script").count()).toBe(0);
      expect(await driver.cloudAppsDialogSetupHref()).toBe("https://crm.example.com/keys");
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-027"], "empty or malformed values block submit inline"),
  { tag: specTags(["CLOUD-027"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    let posts = 0;
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([MANUAL_APP]) }),
      authStatus: () =>
        Promise.resolve({ status: 200, body: { type: "api_key", status: "needs_auth", manualSetup: MANIFEST } }),
      apiKey: () => {
        posts += 1;
        return Promise.resolve({ status: 200, body: { connected: true } });
      },
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Manual CRM"]);
      await driver.cloudAppsClickCardAction("Manual CRM", "Connect");
      await expect.poll(() => driver.cloudAppsDialogVisible(), { timeout: 60_000 }).toBe(true);
      // Empty submit: every problem surfaces at once, nothing posts.
      await driver.cloudAppsDialogSubmit();
      await expect.poll(() => driver.cloudAppsDialogErrors(), { timeout: 60_000 }).toHaveLength(2);
      expect(posts).toBe(0);
      // Malformed key, valid region: only the key complains, still nothing posts.
      await driver.cloudAppsDialogFill("API key", "nope");
      await driver.cloudAppsDialogFill("Region", "us");
      await driver.cloudAppsDialogSubmit();
      await expect.poll(() => driver.cloudAppsDialogErrors(), { timeout: 60_000 }).toHaveLength(1);
      expect(posts).toBe(0);
      expect(await driver.cloudAppsDialogVisible()).toBe(true);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-027"], "submit posts trimmed values, clears secrets, and closes"),
  { tag: specTags(["CLOUD-027"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const bodies: (string | null)[] = [];
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([MANUAL_APP]) }),
      authStatus: () =>
        Promise.resolve({ status: 200, body: { type: "api_key", status: "needs_auth", manualSetup: MANIFEST } }),
      apiKey: (_route, logged) => {
        bodies.push(logged.postData);
        return Promise.resolve({ status: 200, body: { connected: true } });
      },
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Manual CRM"]);
      await driver.cloudAppsClickCardAction("Manual CRM", "Connect");
      await expect.poll(() => driver.cloudAppsDialogVisible(), { timeout: 60_000 }).toBe(true);
      await driver.cloudAppsDialogFill("API key", "  sk-abcd99  ");
      await driver.cloudAppsDialogFill("Region", "us");
      const readsBefore = stub.log.filter((entry) => entry.path.startsWith("/api/openhub/apps/marketplace/")).length;
      await driver.cloudAppsDialogSubmit();
      await expect.poll(() => driver.cloudAppsDialogVisible(), { timeout: 60_000 }).toBe(false);
      expect(bodies).toHaveLength(1);
      expect(JSON.parse(bodies[0] ?? "{}")).toEqual({ fields: { api_key: "sk-abcd99", region: "us" } });
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      expect((await driver.lastToast()) ?? "").toContain("Connected");
      expect(await driver.currentPath()).not.toContain("sk-abcd99");
      // The catalog re-reads after the connect.
      await expect
        .poll(() => stub.log.filter((entry) => entry.path.startsWith("/api/openhub/apps/marketplace/")).length, {
          timeout: 60_000,
        })
        .toBeGreaterThan(readsBefore);
      // Reopening shows cleared fields: no secret lingers in the form.
      await driver.cloudAppsClickCardAction("Manual CRM", "Connect");
      await expect.poll(() => driver.cloudAppsDialogVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.cloudAppsDialogInputValues()).toEqual(["", ""]);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-027"], "issuance link renders only for safely absolute HTTPS"),
  { tag: specTags(["CLOUD-027"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    let setupUrl: string | undefined = "https://crm.example.com/keys";
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([MANUAL_APP]) }),
      authStatus: () =>
        Promise.resolve({
          status: 200,
          body: {
            type: "api_key",
            status: "needs_auth",
            manualSetup: { ...MANIFEST, setupUrl },
          },
        }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Manual CRM"]);
      for (const candidate of [
        { url: "https://crm.example.com/keys", expectHref: "https://crm.example.com/keys" },
        { url: "http://crm.example.com/keys", expectHref: null },
        { url: "https://user:pass@crm.example.com/keys", expectHref: null },
        { url: "/relative/keys", expectHref: null },
      ]) {
        setupUrl = candidate.url;
        await driver.page.reload({ waitUntil: "domcontentloaded" });
        await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Manual CRM"]);
        await driver.cloudAppsClickCardAction("Manual CRM", "Connect");
        await expect.poll(() => driver.cloudAppsDialogVisible(), { timeout: 60_000 }).toBe(true);
        expect(await driver.cloudAppsDialogSetupHref()).toBe(candidate.expectHref);
        await driver.cloudAppsDialogCancel();
        await expect.poll(() => driver.cloudAppsDialogVisible(), { timeout: 60_000 }).toBe(false);
      }
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-027"], "bare api_key falls back to one field and failures stay open"),
  { tag: specTags(["CLOUD-027"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    // Live server state: empty field posts are rejected before any
    // gateway call.
    const empty = await cloudApiKeyRaw(session.cookie, "app-manual", {});
    expect(empty.status).toBe(400);
    expect(empty.payload["error"]).toBe("fields_required");

    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: () => Promise.resolve({ status: 200, body: marketplacePage([MANUAL_APP]) }),
      authStatus: () => Promise.resolve({ status: 200, body: { type: "api_key", status: "needs_auth" } }),
      apiKey: () =>
        Promise.resolve({
          status: 400,
          body: { error: "openhub_invalid_argument", detail: "That credential was rejected." },
        }),
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Manual CRM"]);
      await driver.cloudAppsClickCardAction("Manual CRM", "Connect");
      await expect.poll(() => driver.cloudAppsDialogVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.cloudAppsDialogFields()).toEqual([
        { label: "API key", inputType: "password", placeholder: "" },
      ]);
      await driver.cloudAppsDialogFill("API key", "sk-wrong1");
      await driver.cloudAppsDialogSubmit();
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).not.toBeNull();
      const toast = (await driver.lastToast()) ?? "";
      expect(toast).toContain("Could not connect");
      expect(toast).toContain("That credential was rejected.");
      expect(await driver.cloudAppsDialogVisible()).toBe(true);
    } finally {
      await stub.uninstall();
    }
  }
);
