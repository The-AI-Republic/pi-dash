// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: attachments widget (NEWFRONT-121, Part C; success paths
// restored by NEWFRONT-143 once the parity stack shipped MinIO storage).
// Uploads complete end to end: metadata POST, presigned bytes POST through
// the proxy origin, status PATCH. Client-side rejections (multi-file,
// oversize) and the server 400 for non-whitelisted types behave normally.
// Against scenario-owned issues, green on apps/web first.
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
  specTitle(["ISS-188", "ISS-187"], "allowed uploads succeed and render rows; rejections work"),
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
      await test.step("an allowed upload succeeds and renders its row", async () => {
        await driver.uploadAttachment({ name: fileName, mime: "image/png", bytes: PIXEL_PNG });
        await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).toContain("Attachment uploaded");
        // The in-flight row shows the bare name; the settled row carries the
        // human size, so waiting for both pins the completed render.
        await expect
          .poll(async () => (await driver.widgetRowNames("Attachments")).find((row) => row.includes(fileName)) ?? "", {
            timeout: 60_000,
          })
          .toContain("0 KB");
        const rows = await driver.widgetRowNames("Attachments");
        expect(rows.find((row) => row.includes(fileName)) ?? "").toContain("0 KB");
        const assets = await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(assets).toHaveLength(1);
        const attributes = assets[0]["attributes"] as Record<string, unknown>;
        expect(String(attributes["name"] ?? "")).toBe(fileName);
      });
      await test.step("a server-rejected type surfaces an error", async () => {
        await driver.uploadAttachment({
          name: `oracle-nope-${ts}.txt`,
          mime: "text/plain",
          bytes: Buffer.from("nope"),
        });
        // The rebuild applies no file-type restriction (area spec §15: the
        // old stack's signature-mismatch rejection has no counterpart), so
        // the new leg proves the .txt uploads like any other file while the
        // oracle keeps pinning its rejection (spec §10 gate alignment).
        if (driver.target === "web_new") {
          await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).toContain("Attachment uploaded");
          const assets = await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session);
          expect(assets).toHaveLength(2);
          const names = assets.map((row) =>
            String((row["attributes"] as Record<string, unknown> | undefined)?.["name"] ?? "")
          );
          expect(names).toContain(`oracle-nope-${ts}.txt`);
        } else {
          await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toMatch(/file could not be attached/i);
          const assets = await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session);
          expect(assets.map((row) => String(row["name"] ?? ""))).not.toContain(`oracle-nope-${ts}.txt`);
        }
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-189"], "opening an attachment downloads its bytes in a new tab"),
  { tag: specTags(["ISS-189"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const fileName = `oracle-open-${ts}.png`;
    const issue = await ownIssue(seed, session, `Oracle attach open ${ts}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await driver.uploadAttachment({ name: fileName, mime: "image/png", bytes: PIXEL_PNG });
      await expect
        .poll(async () => (await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session)).length, {
          timeout: 60_000,
        })
        .toBe(1);
      // The row opens the file in a new browser tab, which serves the bytes
      // as a download under the original filename.
      const opened = await driver.downloadWidgetRowFile("Attachments", fileName);
      expect(opened.fileName).toBe(fileName);
      expect(opened.bytes.equals(PIXEL_PNG)).toBe(true);
      const rowsAfter = await driver.widgetRowNames("Attachments");
      expect(rowsAfter.some((row) => row.includes(fileName))).toBe(true);
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-190"], "deleting an attachment confirms and removes it"),
  { tag: specTags(["ISS-190"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const ts = Date.now();
    const fileName = `oracle-del-${ts}.png`;
    const issue = await ownIssue(seed, session, `Oracle attach del ${ts}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await driver.uploadAttachment({ name: fileName, mime: "image/png", bytes: PIXEL_PNG });
      await expect
        .poll(async () => (await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session)).length, {
          timeout: 60_000,
        })
        .toBe(1);
      // Gate on the settled row (name plus human size): the row menu mounts
      // with it, so the delete click below never catches the row mid-mount.
      // (Reading the menu first would toggle it closed on the next click.)
      await expect
        .poll(async () => (await driver.widgetRowNames("Attachments")).find((row) => row.includes(fileName)) ?? "", {
          timeout: 60_000,
        })
        .toContain("0 KB");
      await driver.clickWidgetRowAction("Attachments", fileName, "Delete");
      expect(await driver.confirmModalTitle()).toBe("Delete attachment");
      const modalText = await driver.confirmModalText();
      // The confirmation names the file without its extension.
      expect(modalText).toContain(`oracle-del-${ts}`);
      expect(modalText).toContain("permanently removed");
      await driver.confirmModal("Delete");
      await expect
        .poll(async () => (await issueAttachments(seed.workspaceSlug, seed.projectId, issue.id, session)).length, {
          timeout: 60_000,
        })
        .toBe(0);
      expect(await driver.widgetRowNames("Attachments")).toHaveLength(0);
      await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).toContain("Attachment removed");
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
