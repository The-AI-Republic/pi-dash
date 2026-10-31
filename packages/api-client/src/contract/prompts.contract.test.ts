// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: prompt-section reads, workspace and personal override
// upserts plus reverts, the compiled receipt and render previews
// against live Django. Override tests self-clean through the revert
// endpoint, so a dead run's leftover is overwritten, never orphaned.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { listIssues } from "../contracts/issues.js";
import {
  getCompiledPrompt,
  listPromptSections,
  previewPrompt,
  revertPromptSection,
  upsertPromptSection,
} from "../contracts/prompts.js";
import {
  createScheduler,
  createSchedulerBinding,
  deleteScheduler,
  deleteSchedulerBinding,
} from "../contracts/schedulers.js";
import { listProjects } from "../contracts/projects.js";
import { getMe } from "../contracts/users.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function seededIssueId(client: ApiClient, projectId: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const first = page.results[0];
  expect(first).toBeDefined();
  return first?.id ?? "";
}

/** An overridable section of the coding-task recipe for write tests. */
const OVERRIDABLE_KEY = "autonomy";

let promptNonce = 0;
function overrideBody(): string {
  promptNonce += 1;
  return `Contract override ${Date.now().toString(36)}-${promptNonce}. Plain prose renders in every sample context.`;
}

describe.skipIf(!contractEnabled)("prompt sections contract", () => {
  it("lists the default kind at user scope", async () => {
    const client = await signedIn();
    const list = await listPromptSections(client, CONTRACT_WORKSPACE);
    expect(list.kind).toBe("coding-task");
    expect(list.scope).toBe("user");
    expect(list.sections.length).toBeGreaterThan(0);
    const keys = list.sections.map((section) => section.key);
    expect(keys).toContain("intro");
    expect(keys).toContain(OVERRIDABLE_KEY);
    const autonomy = list.sections.find((section) => section.key === OVERRIDABLE_KEY);
    expect(autonomy?.customizable).toBe("overridable");
    expect(autonomy?.editable_at_workspace).toBe(true);
    expect(autonomy?.editable_at_personal).toBe(true);
    expect(autonomy?.default_body.length).toBeGreaterThan(0);
  });

  it("lists the scheduler kind at workspace scope", async () => {
    const client = await signedIn();
    const list = await listPromptSections(client, CONTRACT_WORKSPACE, { kind: "scheduler", scope: "workspace" });
    expect(list.kind).toBe("scheduler");
    expect(list.scope).toBe("workspace");
    expect(list.sections.length).toBeGreaterThan(0);
  });

  it("rejects an unknown kind with 400", async () => {
    const client = await signedIn();
    const unknown = await listPromptSections(client, CONTRACT_WORKSPACE, { kind: "nope" }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(unknown).toBeInstanceOf(ApiError);
    expect((unknown as ApiError).status).toBe(400);
  });
});

describe.skipIf(!contractEnabled)("prompt override contract", () => {
  it("upserts and reverts a workspace override", async () => {
    const client = await signedIn();
    const body = overrideBody();
    const stored = await upsertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, {
      scope: "workspace",
      body,
    });
    expect(stored.section_key).toBe(OVERRIDABLE_KEY);
    expect(stored.body).toBe(body);
    expect(stored.is_active).toBe(true);
    expect(stored.is_workspace_level).toBe(true);
    expect(stored.user).toBeNull();
    try {
      const list = await listPromptSections(client, CONTRACT_WORKSPACE, {
        kind: "coding-task",
        scope: "workspace",
      });
      const row = list.sections.find((section) => section.key === OVERRIDABLE_KEY);
      expect(row?.source).toBe("workspace");
      expect(row?.body).toBe(body);
    } finally {
      await revertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, "workspace").catch(() => 0);
    }
    const reverted = await listPromptSections(client, CONTRACT_WORKSPACE, {
      kind: "coding-task",
      scope: "workspace",
    });
    const row = reverted.sections.find((section) => section.key === OVERRIDABLE_KEY);
    expect(row?.source).toBe("default");
  });

  it("upserts and reverts a personal override", async () => {
    const client = await signedIn();
    const me = await getMe(client);
    const body = overrideBody();
    const stored = await upsertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, {
      scope: "user",
      body,
    });
    expect(stored.is_workspace_level).toBe(false);
    expect(stored.user).toBe(me.id);
    try {
      const list = await listPromptSections(client, CONTRACT_WORKSPACE, { kind: "coding-task" });
      const row = list.sections.find((section) => section.key === OVERRIDABLE_KEY);
      expect(row?.source).toBe(`user:${me.id}`);
      expect(row?.body).toBe(body);
    } finally {
      await revertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, "user").catch(() => 0);
    }
  });

  it("refuses a locked section with 403", async () => {
    const client = await signedIn();
    const locked = await upsertPromptSection(client, CONTRACT_WORKSPACE, "intro", {
      scope: "workspace",
      body: "Contract locked write.",
    }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(locked).toBeInstanceOf(ApiError);
    expect((locked as ApiError).status).toBe(403);
  });

  it("reverting a scope with no override 404s", async () => {
    const client = await signedIn();
    await revertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, "workspace").catch(() => 0);
    const missing = await revertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, "workspace").then(
      () => null,
      (caught: unknown) => caught
    );
    expect(missing).toBeInstanceOf(ApiError);
    expect((missing as ApiError).status).toBe(404);
  });
});

describe.skipIf(!contractEnabled)("compiled prompt contract", () => {
  it("assembles the workspace template without the automatic variant", async () => {
    const client = await signedIn();
    const compiled = await getCompiledPrompt(client, CONTRACT_WORKSPACE, "coding-task", { scope: "workspace" });
    expect(compiled.kind).toBe("coding-task");
    expect(compiled.scope).toBe("workspace");
    expect(compiled.template_body.length).toBeGreaterThan(0);
    expect(compiled.automatic_template_body).toBeUndefined();
  });

  it("adds the automatic variant while a personal override exists", async () => {
    const client = await signedIn();
    // A personal override must be the only delta: clear both scopes first
    // so the variant below is attributable to this test's row.
    await revertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, "user").catch(() => 0);
    await revertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, "workspace").catch(() => 0);
    const before = await getCompiledPrompt(client, CONTRACT_WORKSPACE, "coding-task");
    expect(before.automatic_template_body).toBeUndefined();
    const body = overrideBody();
    await upsertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, { scope: "user", body });
    try {
      const during = await getCompiledPrompt(client, CONTRACT_WORKSPACE, "coding-task");
      expect(during.scope).toBe("user");
      expect(during.template_body).toContain(body);
      expect(during.automatic_template_body).toBeDefined();
      expect(during.automatic_template_body).not.toContain(body);
    } finally {
      await revertPromptSection(client, CONTRACT_WORKSPACE, OVERRIDABLE_KEY, "user").catch(() => 0);
    }
    const after = await getCompiledPrompt(client, CONTRACT_WORKSPACE, "coding-task");
    expect(after.automatic_template_body).toBeUndefined();
  });
});

describe.skipIf(!contractEnabled)("prompt preview contract", () => {
  it("renders a saved prompt against a real issue", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const preview = await previewPrompt(client, CONTRACT_WORKSPACE, "coding-task", {
      scope: "workspace",
      issue_id: await seededIssueId(client, projectId),
    });
    expect(preview.kind).toBe("coding-task");
    expect(preview.prompt.length).toBeGreaterThan(0);
  });

  it("renders an unsaved draft in place of its section", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const marker = `contract-draft-${Date.now().toString(36)}`;
    const preview = await previewPrompt(client, CONTRACT_WORKSPACE, "coding-task", {
      scope: "user",
      issue_id: await seededIssueId(client, projectId),
      section_key: OVERRIDABLE_KEY,
      body: `Draft preview ${marker}.`,
    });
    expect(preview.prompt).toContain(marker);
  });

  it("renders the scheduler kind against a real install", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const slug = `contract-prev-${Date.now().toString(36)}`;
    const definition = await createScheduler(client, CONTRACT_WORKSPACE, {
      name: `Contract ${slug}`,
      slug,
      prompt: "Contract base prompt.",
    });
    const installed = await createSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, {
      scheduler: definition.id,
      dtstart: new Date().toISOString(),
      rrule: "FREQ=DAILY",
    });
    try {
      const preview = await previewPrompt(client, CONTRACT_WORKSPACE, "scheduler", {
        binding_id: installed.id,
      });
      expect(preview.kind).toBe("scheduler");
      expect(preview.prompt.length).toBeGreaterThan(0);
    } finally {
      await deleteSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, installed.id).catch(() => 0);
      await deleteScheduler(client, CONTRACT_WORKSPACE, definition.id).catch(() => 0);
    }
  });

  it("requires a target for the kind with 400", async () => {
    const client = await signedIn();
    const missing = await previewPrompt(client, CONTRACT_WORKSPACE, "coding-task", {}).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(missing).toBeInstanceOf(ApiError);
    expect((missing as ApiError).status).toBe(400);
  });
});
