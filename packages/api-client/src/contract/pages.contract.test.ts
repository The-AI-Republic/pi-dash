// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the core pages REST surface against live Django (NEWFRONT-833,
// parity/specs/pages.md §11.1–§11.5). Every write runs on a probe page
// created for this suite and archived then deleted in finally, so the
// seeded "Contract page" is never mutated. The nested fields and the
// `parent` children read are the backend delta recorded in
// parity/api-client-contracts.md (NEWFRONT-850 / PDASHOSS01-270), so this
// suite deliberately asserts only the live surface: the current list
// ignores `parent` and answers top-level rows.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  addPageFavorite,
  archivePage,
  createPage,
  deletePage,
  duplicatePage,
  getPage,
  getPageDescriptionBinary,
  getPageVersion,
  listPageVersions,
  listPages,
  lockPage,
  movePageToProject,
  removePageFavorite,
  restorePage,
  setPageAccess,
  unlockPage,
  updatePage,
  updatePageDescription,
} from "../contracts/pages.js";
import {
  attachPageAssets,
  checkPageAsset,
  completePageAsset,
  duplicatePageAsset,
  pageAssetDownloadUrl,
  pageAssetUrl,
  requestPageAsset,
} from "../contracts/page-assets.js";
import { listProjects } from "../contracts/projects.js";
import { searchEntityPreviews, searchMentionPeople } from "../contracts/entity-search.js";
import {
  CONTRACT_BASE_URL,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_WORKSPACE,
  contractEnabled,
  sharedSignedIn,
} from "./setup.js";
import { createClient } from "../client.js";
import { signIn } from "../contracts/auth-entry.js";
import { contractTransport, CONTRACT_EMAIL, CONTRACT_PASSWORD } from "./setup.js";

const PROBE = "Contract pages probe";
const MARK = { in_use: "emoji", emoji: { value: "1f600" } } as const;

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

/** Archive then delete, swallowing the not-found a cleanup may race into. */
async function removeProbe(client: ApiClient, projectId: string, pageId: string): Promise<void> {
  await archivePage(client, CONTRACT_WORKSPACE, projectId, pageId).catch(() => null);
  await deletePage(client, CONTRACT_WORKSPACE, projectId, pageId).catch(() => null);
}

/** The seeded "Contract page" (its snapshot row is seeded with it). */
async function seededPageId(client: ApiClient, projectId: string): Promise<string> {
  const rows = await listPages(client, CONTRACT_WORKSPACE, projectId);
  const seeded = rows.find((row) => row.name === "Contract page");
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("pages REST contract", () => {
  it("creates, lists and reads a page in every live shape", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, {
      name: `${PROBE} create`,
      access: 0,
      logo_props: MARK,
      description_html: "<p>Probe body</p>",
    });
    try {
      expect(created.name).toBe(`${PROBE} create`);
      expect(created.access).toBe(0);
      expect(created.parent).toBeNull();
      expect(created.logo_props.in_use).toBe("emoji");
      expect(created.description_html).toContain("Probe body");

      // The nested delta: the live list ignores `parent` and answers the
      // top-level collection, the page is in it, and no nested fields yet.
      const rows = await listPages(client, CONTRACT_WORKSPACE, projectId);
      const listed = rows.find((row) => row.id === created.id);
      expect(listed).toBeDefined();
      expect(listed?.is_favorite).toBe(false);
      expect(listed?.project_ids).toContain(projectId);
      expect(listed?.parent_id).toBeUndefined();
      expect(listed?.sub_pages_count).toBeUndefined();
      // Until PDASHOSS01-270 the live list ignores `parent` and answers the
      // same top-level collection, so a children read is not a child list.
      const byParent = await listPages(client, CONTRACT_WORKSPACE, projectId, { parent: created.id });
      expect(byParent.map((row) => row.id).sort()).toEqual(rows.map((row) => row.id).sort());

      const searched = await listPages(client, CONTRACT_WORKSPACE, projectId, { search: `${PROBE} create` });
      expect(searched.map((row) => row.id)).toContain(created.id);

      const detail = await getPage(client, CONTRACT_WORKSPACE, projectId, created.id, { trackVisit: false });
      expect(detail.id).toBe(created.id);
      expect(detail.name).toBe(`${PROBE} create`);
      expect(detail.description_html).toContain("Probe body");
      expect(detail.issue_ids).toEqual([]);
      expect(detail.label_ids).toEqual([]);
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });

  it("patches title, mark, visibility, parent and lock", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} patch` });
    try {
      const renamed = await updatePage(client, CONTRACT_WORKSPACE, projectId, created.id, {
        name: `${PROBE} renamed`,
        logo_props: MARK,
      });
      expect(renamed.id).toBe(created.id);
      expect(renamed.name).toBe(`${PROBE} renamed`);
      expect(renamed.logo_props?.emoji?.value).toBe("1f600");

      const cleared = await updatePage(client, CONTRACT_WORKSPACE, projectId, created.id, {
        parent: null,
        logo_props: {},
      });
      expect(cleared.logo_props).toEqual({});
      expect(cleared.parent).toBeNull();

      const privatePage = await updatePage(client, CONTRACT_WORKSPACE, projectId, created.id, { access: 1 });
      expect(privatePage.access).toBe(1);
      const publicPage = await updatePage(client, CONTRACT_WORKSPACE, projectId, created.id, { access: 0 });
      expect(publicPage.access).toBe(0);

      await lockPage(client, CONTRACT_WORKSPACE, projectId, created.id);
      const locked = await getPage(client, CONTRACT_WORKSPACE, projectId, created.id, { trackVisit: false });
      expect(locked.is_locked).toBe(true);
      await expect(
        updatePage(client, CONTRACT_WORKSPACE, projectId, created.id, { name: `${PROBE} frozen` })
      ).rejects.toMatchObject({ status: 400 });
      await expect(
        updatePageDescription(client, CONTRACT_WORKSPACE, projectId, created.id, {
          description_html: "<p>frozen</p>",
        })
      ).rejects.toMatchObject({ status: 400, body: { error_message: "PAGE_LOCKED" } });

      await unlockPage(client, CONTRACT_WORKSPACE, projectId, created.id);
      const unlocked = await getPage(client, CONTRACT_WORKSPACE, projectId, created.id, { trackVisit: false });
      expect(unlocked.is_locked).toBe(false);
      const afterUnlock = await updatePage(client, CONTRACT_WORKSPACE, projectId, created.id, {
        name: `${PROBE} patch`,
      });
      expect(afterUnlock.name).toBe(`${PROBE} patch`);
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });

  it("switches visibility through the access route", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} access` });
    try {
      await setPageAccess(client, CONTRACT_WORKSPACE, projectId, created.id, 1);
      const restricted = await listPages(client, CONTRACT_WORKSPACE, projectId);
      expect(restricted.find((row) => row.id === created.id)?.access).toBe(1);
      await setPageAccess(client, CONTRACT_WORKSPACE, projectId, created.id, 0);
      const open = await listPages(client, CONTRACT_WORKSPACE, projectId);
      expect(open.find((row) => row.id === created.id)?.access).toBe(0);
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });

  it("stars and unstars a page and reads the flag back", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} favorite` });
    try {
      await addPageFavorite(client, CONTRACT_WORKSPACE, projectId, created.id);
      const starred = await listPages(client, CONTRACT_WORKSPACE, projectId);
      expect(starred.find((row) => row.id === created.id)?.is_favorite).toBe(true);
      await removePageFavorite(client, CONTRACT_WORKSPACE, projectId, created.id);
      const unstarred = await listPages(client, CONTRACT_WORKSPACE, projectId);
      expect(unstarred.find((row) => row.id === created.id)?.is_favorite).toBe(false);
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });

  it("archives, restores and deletes in the order the server enforces", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, {
      name: `${PROBE} archive`,
      description_html: "<p>shelved</p>",
    });
    let deleted = false;
    try {
      await expect(deletePage(client, CONTRACT_WORKSPACE, projectId, created.id)).rejects.toMatchObject({
        status: 400,
      });

      const stamp = await archivePage(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(stamp.archived_at).toContain("2026-");
      // The collection keeps shelved rows and stamps `archived_at`; the
      // pages tab partitions on that stamp rather than on absence.
      const rows = await listPages(client, CONTRACT_WORKSPACE, projectId);
      expect(rows.find((row) => row.id === created.id)?.archived_at).not.toBeNull();
      await expect(
        updatePageDescription(client, CONTRACT_WORKSPACE, projectId, created.id, {
          description_html: "<p>no</p>",
        })
      ).rejects.toMatchObject({ status: 400, body: { error_message: "PAGE_ARCHIVED" } });

      await restorePage(client, CONTRACT_WORKSPACE, projectId, created.id);
      const restored = await getPage(client, CONTRACT_WORKSPACE, projectId, created.id, { trackVisit: false });
      expect(restored.archived_at).toBeNull();
      expect(restored.description_html).toContain("shelved");

      await archivePage(client, CONTRACT_WORKSPACE, projectId, created.id);
      await deletePage(client, CONTRACT_WORKSPACE, projectId, created.id);
      deleted = true;
      await expect(
        getPage(client, CONTRACT_WORKSPACE, projectId, created.id, { trackVisit: false })
      ).rejects.toMatchObject({ status: 404 });
    } finally {
      if (!deleted) await removeProbe(client, projectId, created.id);
    }
  });

  it("records the community build's missing move route as a 404", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} move` });
    try {
      // Cloud editions serve POST .../move/; the community build has no
      // route at all, so the live leg pins the 404 absence (PAGE-017).
      await expect(
        movePageToProject(client, CONTRACT_WORKSPACE, projectId, created.id, { new_project_id: projectId })
      ).rejects.toMatchObject({ status: 404 });
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });

  it("duplicates a page into a detached copy", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, {
      name: `${PROBE} duplicate`,
      access: 1,
      logo_props: MARK,
      description_html: "<p>copy me</p>",
    });
    let copyId: string | null = null;
    try {
      const copy = await duplicatePage(client, CONTRACT_WORKSPACE, projectId, created.id);
      copyId = copy.id;
      expect(copy.id).not.toBe(created.id);
      expect(copy.name).toBe(`${PROBE} duplicate (Copy)`);
      expect(copy.access).toBe(1);
      expect(copy.logo_props.emoji?.value).toBe("1f600");
      expect(copy.parent).toBeNull();
      expect(copy.description_html).toContain("copy me");
    } finally {
      await removeProbe(client, projectId, created.id);
      if (copyId) await removeProbe(client, projectId, copyId);
    }
  });

  it("reads the binary body and writes the three description forms", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} body` });
    try {
      const empty = await getPageDescriptionBinary(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(empty).toBeInstanceOf(Uint8Array);

      const written = await updatePageDescription(client, CONTRACT_WORKSPACE, projectId, created.id, {
        description_html: "<p>fallback body</p>",
        description_json: { type: "doc", content: [{ type: "paragraph" }] },
      });
      expect(written.message).toBeTruthy();
      const detail = await getPage(client, CONTRACT_WORKSPACE, projectId, created.id, { trackVisit: false });
      expect(detail.description_html).toContain("fallback body");

      await expect(
        updatePageDescription(client, CONTRACT_WORKSPACE, projectId, created.id, {
          description_html: "<p>x</p>",
          description_binary: null,
        } as never)
      ).rejects.toMatchObject({ status: 400 });
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });

  it("reads the seeded page's version list and one snapshot", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const page = await getPage(client, CONTRACT_WORKSPACE, projectId, await seededPageId(client, projectId), {
      trackVisit: false,
    });
    const versions = await listPageVersions(client, CONTRACT_WORKSPACE, projectId, page.id);
    expect(versions.length).toBeGreaterThan(0);
    const newest = versions[0];
    expect(newest?.page).toBe(page.id);
    expect(newest?.workspace).toBeTruthy();

    const snapshot = await getPageVersion(client, CONTRACT_WORKSPACE, projectId, page.id, newest?.id ?? "");
    expect(snapshot.id).toBe(newest?.id);
    expect(snapshot.page).toBe(page.id);
    expect(snapshot.description_html).toContain("Contract page snapshot");
    expect(snapshot.description_json).toEqual({ type: "doc", content: [{ type: "paragraph" }] });
    // No binary was stored for the snapshot; the live detail answers null.
    expect(snapshot.description_binary).toBeNull();
  });

  it("answers an empty version list for a page with no snapshots yet", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, {
      name: `${PROBE} versions`,
      description_html: "<p>v1</p>",
    });
    try {
      const versions = await listPageVersions(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(versions).toEqual([]);
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });

  it("refuses a create with a parent the live serializer cannot resolve and records no nested read", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const parent = await createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} parent` });
    try {
      await expect(
        createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} child`, parent: parent.id })
      ).rejects.toMatchObject({ status: 404 });
      // The live backend accepted the nested parameter (the child row was
      // minted) but its serializer cannot resolve the parent pk, so the
      // request answers 404. Record which row exists so the cleanup can
      // find it even though the caller never learned its id.
      const rows = await listPages(client, CONTRACT_WORKSPACE, projectId);
      const strayChild = rows.find((row) => row.name === `${PROBE} child`);
      if (strayChild) await removeProbe(client, projectId, strayChild.id);
    } finally {
      await removeProbe(client, projectId, parent.id);
    }
  });
});

describe.skipIf(!contractEnabled)("workspace entity search contract", () => {
  it("answers the people and page sections the editor asks for", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} searchable` });
    try {
      const people = await searchMentionPeople(client, CONTRACT_WORKSPACE, {
        query: "contract",
        projectId,
        count: 5,
      });
      expect(people.length).toBeGreaterThan(0);
      expect(people[0]?.member__id ?? "").toBeTruthy();
      expect(typeof (people[0]?.member__display_name ?? null)).toBe("string");

      const result = await searchEntityPreviews(client, CONTRACT_WORKSPACE, {
        query: `${PROBE} searchable`,
        kinds: ["page", "project"],
        projectId,
        count: 10,
      });
      expect(result.page?.map((row) => row.id)).toContain(created.id);
      expect(result.page?.find((row) => row.id === created.id)?.projects__id).toBe(projectId);
      expect(result.issue).toBeUndefined();
      expect(result.cycle).toBeUndefined();
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });
});

describe.skipIf(!contractEnabled)("page image asset contract", () => {
  it("parks, confirms, links, clones and reads a page image", async () => {
    const client = await sharedSignedIn();
    const probeJar = new Map<string, string>();
    const redirectFree = createClient({ baseUrl: CONTRACT_BASE_URL, transport: contractTransport(probeJar) });
    expect((await signIn(redirectFree, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD })).ok).toBe(true);
    const projectId = await seededProjectId(client);
    const created = await createPage(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} assets` });
    try {
      const file = new File([new Uint8Array([7, 7, 7, 7])], "contract-page.png", { type: "image/png" });
      const grant = await requestPageAsset(client, CONTRACT_WORKSPACE, projectId, created.id, file);
      expect(grant.asset_id).toBeTruthy();
      expect(grant.upload_data.url).toBeTruthy();
      expect(grant.upload_data.fields).toBeTypeOf("object");

      await completePageAsset(client, CONTRACT_WORKSPACE, projectId, grant.asset_id);
      await attachPageAssets(client, CONTRACT_WORKSPACE, projectId, created.id, [grant.asset_id]);
      expect((await checkPageAsset(client, CONTRACT_WORKSPACE, grant.asset_id)).exists).toBe(true);

      const clone = await duplicatePageAsset(client, CONTRACT_WORKSPACE, projectId, grant.asset_id, created.id);
      expect(clone.asset_id).not.toBe(grant.asset_id);
      expect((await checkPageAsset(client, CONTRACT_WORKSPACE, clone.asset_id)).exists).toBe(true);

      // The view leg is a redirect onto the object store. Read the raw
      // response with the suite's cookie jar: the client follows redirects
      // and would land on the dummy store instead of seeing the 302.
      const cookie = [...probeJar.entries()].map(([name, value]) => `${name}=${value}`).join("; ");
      const served = await globalThis.fetch(
        `${CONTRACT_BASE_URL}${pageAssetUrl(CONTRACT_WORKSPACE, projectId, grant.asset_id)}`,
        { headers: { Cookie: cookie }, redirect: "manual" }
      );
      expect(served.status).toBe(302);
      expect(served.headers.get("location")?.length ?? 0).toBeGreaterThan(0);
      expect(pageAssetDownloadUrl(CONTRACT_WORKSPACE, projectId, grant.asset_id)).toContain("/download/");
    } finally {
      await removeProbe(client, projectId, created.id);
    }
  });
});
