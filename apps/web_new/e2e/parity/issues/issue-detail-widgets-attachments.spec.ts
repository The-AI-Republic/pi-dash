// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: attachments widget (NEWFRONT-121, Part C).
// bug: NEWFRONT-143 — the parity stack configures no S3 bucket, so the
// metadata POST 500s and no upload can succeed; success/open/delete paths
// assert the observed failure behavior. Client-side rejections
// (multi-file, oversize) and the server 400 for non-whitelisted types
// behave normally and are green. Against scenario-owned issues, green on
// apps/web first.
// Rows: ISS-187, ISS-188, ISS-189, ISS-190, ISS-191.
import { test, expect } from "../fixtures";
import { instanceConfig, issueAttachments, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

// A 1x1 transparent PNG: the server enforces a MIME whitelist (images,
// documents), so uploads use an allowed type. The client itself sets no
// accept filter.
const PIXEL_PNG = Buffer.from(
  "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000a49444154789c63000100000500010d0a2db40000000049454e44ae426082",
  "hex"
);

test(
  specTitle(["ISS-188", "ISS-187"], "bug: NEWFRONT-143 uploads fail without stack storage; rejections work"),
  { tag: specTags(["ISS-188", "ISS-187"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle attach up ${ts}`);
    const fileName = `oracle-note-${ts}.png`;
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      // First: the multi-file drop needs a quiescent dropzone (an in-flight
      // upload disables it), so it runs before any upload attempt.
      await test.step("two files at once are rejected", async () => {
        // The picker input is single-file, so the >1 path is only reachable
        // through a real multi-file drop: feed two files to the dropzone
        // root (the Attach button) via a synthetic drop event.
        await driver.page.evaluate((payload: string) => {
          const bytes = Uint8Array.from(atob(payload), (c) => c.charCodeAt(0));
          const dt = new DataTransfer();
          dt.items.add(new File([bytes], "oracle-a.png", { type: "image/png" }));
          dt.items.add(new File([bytes], "oracle-b.png", { type: "image/png" }));
          const input = document.querySelector('input[type="file"]');
          const root = input?.parentElement ?? document.body;
          for (const type of ["dragenter", "dragover", "drop"]) {
            root.dispatchEvent(new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true }));
          }
        }, PIXEL_PNG.toString("base64"));
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("Only one file");
        const assets = await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(assets.map((row) => String(row["name"] ?? ""))).not.toContain(`oracle-a-${ts}.png`);
      });
      await test.step("bug: an allowed upload fails and renders no row", async () => {
        // Intended: the row renders with icon, filename, size, and uploader
        // avatar, plus a success toast and a server-side asset.
        // Observed: the dropzone raises its own failure toast over the
        // shared operation's promise toast, so this text wins.
        await driver.uploadAttachment({ name: fileName, mime: "image/png", bytes: PIXEL_PNG });
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toMatch(/file could not be attached/i);
        expect(await driver.widgetTitles()).not.toContain("Attachments");
        expect(await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session)).toHaveLength(0);
      });
      await test.step("a server-rejected type surfaces an error", async () => {
        await driver.uploadAttachment({
          name: `oracle-nope-${ts}.txt`,
          mime: "text/plain",
          bytes: Buffer.from("nope"),
        });
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toMatch(/file could not be attached/i);
        const assets = await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(assets.map((row) => String(row["name"] ?? ""))).not.toContain(`oracle-nope-${ts}.txt`);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-189"], "bug: NEWFRONT-143 no attachment to open without storage"),
  { tag: specTags(["ISS-189"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle attach open ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      // Intended: clicking an attachment row opens its asset_url in a new
      // browser tab. Unreachable while uploads fail.
      expect(await driver.widgetTitles()).not.toContain("Attachments");
      expect(await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session)).toHaveLength(0);
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-190"], "bug: NEWFRONT-143 no attachment to delete without storage"),
  { tag: specTags(["ISS-190"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle attach del ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      // Intended: Delete asks for confirmation naming the file ("Delete
      // attachment ... permanently removed"), then removes the row and the
      // asset with an "Attachment removed" toast. Unreachable while uploads fail.
      expect(await driver.widgetTitles()).not.toContain("Attachments");
      expect(await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session)).toHaveLength(0);
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-191"], "oversize uploads are rejected with the size limit"),
  { tag: specTags(["ISS-191"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const issue = await ownIssue(seed, session, `Oracle attach big ${ts}`);
    try {
      const config = await instanceConfig(undefined, session);
      const limit = Number(config["file_size_limit"] ?? 0);
      expect(limit, "instance file_size_limit").toBeGreaterThan(0);
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await driver.uploadAttachment({
        name: `oracle-big-${ts}.png`,
        mime: "image/png",
        bytes: Buffer.alloc(limit + 1),
      });
      await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain(`${limit / 1024 / 1024}MB`);
      expect(await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session)).toHaveLength(0);
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);
