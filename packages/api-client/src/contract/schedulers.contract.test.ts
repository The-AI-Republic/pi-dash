// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: scheduler definitions, project bindings, binding run
// history (parsed as the shared AgentRun page) and the occurrences
// window against live Django, plus the project-update automation
// fields. Throwaway definitions/bindings round-trip through the API;
// the seeded install (contract-runs) proves joined run fields. The
// install dialogs' pod read reuses the runners area's pod list.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { createProject, deleteProject, updateProject } from "../contracts/project-management.js";
import { listProjects } from "../contracts/projects.js";
import { listStates } from "../contracts/reference.js";
import {
  createScheduler,
  createSchedulerBinding,
  deleteScheduler,
  deleteSchedulerBinding,
  getScheduler,
  getSchedulerBinding,
  listBindingRuns,
  listSchedulerBindings,
  listSchedulerOccurrences,
  listSchedulers,
  updateScheduler,
  updateSchedulerBinding,
} from "../contracts/schedulers.js";
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

/** Fresh handle per call: delete frees the handle, but a dead run's leftover must not collide. */
let schedulerNonce = 0;
function schedulerHandle(): string {
  schedulerNonce += 1;
  return `contract-sched-${Date.now().toString(36)}-${schedulerNonce}`;
}

async function seededBindingId(client: ApiClient, projectId: string): Promise<string> {
  const bindings = await listSchedulerBindings(client, CONTRACT_WORKSPACE, projectId);
  const seeded = bindings.find((binding) => binding.scheduler_slug === "contract-runs");
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("scheduler definition contract", () => {
  it("round-trips a definition through create, read, update and delete", async () => {
    const client = await signedIn();
    const slug = schedulerHandle();
    const created = await createScheduler(client, CONTRACT_WORKSPACE, {
      name: `Contract ${slug}`,
      slug,
      description: "Contract definition.",
      prompt: "Contract base prompt.",
      color: "#3b82f6",
    });
    expect(created.slug).toBe(slug);
    expect(created.source).toBe("builtin");
    expect(created.active_binding_count).toBe(0);
    try {
      const read = await getScheduler(client, CONTRACT_WORKSPACE, created.id);
      expect(read.name).toBe(created.name);
      const listed = await listSchedulers(client, CONTRACT_WORKSPACE);
      expect(listed.map((row) => row.id)).toContain(created.id);
      const updated = await updateScheduler(client, CONTRACT_WORKSPACE, created.id, {
        name: `Renamed ${slug}`,
        is_enabled: false,
      });
      expect(updated.name).toBe(`Renamed ${slug}`);
      expect(updated.slug).toBe(slug);
      expect(updated.is_enabled).toBe(false);
    } finally {
      await deleteScheduler(client, CONTRACT_WORKSPACE, created.id).catch(() => 0);
    }
    const gone = await getScheduler(client, CONTRACT_WORKSPACE, created.id).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(gone).toBeInstanceOf(ApiError);
    expect((gone as ApiError).status).toBe(404);
  });

  it("rejects a duplicate handle with 400", async () => {
    const client = await signedIn();
    const slug = schedulerHandle();
    const created = await createScheduler(client, CONTRACT_WORKSPACE, {
      name: `Contract ${slug}`,
      slug,
      prompt: "Contract base prompt.",
    });
    try {
      const clash = await createScheduler(client, CONTRACT_WORKSPACE, {
        name: `Clash ${slug}`,
        slug,
        prompt: "Contract base prompt.",
      }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(clash).toBeInstanceOf(ApiError);
      expect((clash as ApiError).status).toBe(400);
    } finally {
      await deleteScheduler(client, CONTRACT_WORKSPACE, created.id).catch(() => 0);
    }
  });
});

describe.skipIf(!contractEnabled)("scheduler binding contract", () => {
  it("installs, reads, edits, toggles and uninstalls", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const slug = schedulerHandle();
    const definition = await createScheduler(client, CONTRACT_WORKSPACE, {
      name: `Contract ${slug}`,
      slug,
      prompt: "Contract base prompt.",
    });
    const dtstart = new Date(Date.now() + 60 * 60 * 1000).toISOString();
    const installed = await createSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, {
      scheduler: definition.id,
      dtstart,
      tzid: "UTC",
      rrule: "FREQ=DAILY",
      extra_context: "Contract context.",
      outcome_mode: "apply_fix",
    });
    expect(installed.scheduler).toBe(definition.id);
    expect(installed.scheduler_slug).toBe(slug);
    expect(installed.rrule).toBe("FREQ=DAILY");
    expect(installed.pod).toBeNull();
    expect(installed.next_run_at).not.toBeNull();
    try {
      const listed = await listSchedulerBindings(client, CONTRACT_WORKSPACE, projectId);
      expect(listed.map((row) => row.id)).toContain(installed.id);
      const detail = await getSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, installed.id);
      expect(detail.id).toBe(installed.id);
      expect(detail.resolved_prompt.length).toBeGreaterThan(0);
      expect(detail.run_count).toBe(0);
      expect(detail.scheduler_source).toBe("builtin");
      expect(detail.scheduler_is_enabled).toBe(true);
      // Enabled-only PATCH is the list/detail switch; the answer keeps
      // the detail shape.
      const disabled = await updateSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, installed.id, {
        enabled: false,
      });
      expect(disabled.enabled).toBe(false);
      expect(disabled.resolved_prompt.length).toBeGreaterThan(0);
      const edited = await updateSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, installed.id, {
        enabled: true,
        extra_context: "Edited context.",
        outcome_mode: "fix_and_review",
      });
      expect(edited.enabled).toBe(true);
      expect(edited.extra_context).toBe("Edited context.");
      expect(edited.outcome_mode).toBe("fix_and_review");
    } finally {
      await deleteSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, installed.id).catch(() => 0);
      await deleteScheduler(client, CONTRACT_WORKSPACE, definition.id).catch(() => 0);
    }
    const gone = await getSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, installed.id).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(gone).toBeInstanceOf(ApiError);
    expect((gone as ApiError).status).toBe(404);
  });

  it("reads the seeded install with joined last-run fields", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const detail = await getSchedulerBinding(
      client,
      CONTRACT_WORKSPACE,
      projectId,
      await seededBindingId(client, projectId)
    );
    expect(detail.scheduler_slug).toBe("contract-runs");
    expect(detail.last_run).not.toBeNull();
    expect(detail.last_run_status).toBe("completed");
    expect(detail.last_run_ended_at).not.toBeNull();
    expect(detail.run_count).toBeGreaterThanOrEqual(1);
    expect(detail.next_run_at).toBeNull();
  });

  it("rejects an invalid recurrence rule with 400", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const slug = schedulerHandle();
    const definition = await createScheduler(client, CONTRACT_WORKSPACE, {
      name: `Contract ${slug}`,
      slug,
      prompt: "Contract base prompt.",
    });
    try {
      const bad = await createSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, {
        scheduler: definition.id,
        dtstart: new Date().toISOString(),
        rrule: "FREQ=NONSENSE",
      }).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(bad).toBeInstanceOf(ApiError);
      expect((bad as ApiError).status).toBe(400);
    } finally {
      await deleteScheduler(client, CONTRACT_WORKSPACE, definition.id).catch(() => 0);
    }
  });
});

describe.skipIf(!contractEnabled)("binding runs contract", () => {
  it("pages the seeded install's history with full run rows", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const bindingId = await seededBindingId(client, projectId);
    const page = await listBindingRuns(client, CONTRACT_WORKSPACE, projectId, bindingId, {
      page: 1,
      per_page: 10,
    });
    expect(page.total_count).toBeGreaterThanOrEqual(1);
    expect(page.page).toBe(1);
    expect(page.per_page).toBe(10);
    expect(page.count).toBe(page.results.length);
    const first = page.results[0];
    expect(first).toBeDefined();
    if (!first) return;
    expect(first.scheduler_binding).toBe(bindingId);
    expect(first.scheduler_binding_detail?.scheduler_slug).toBe("contract-runs");
    expect(first.pod_detail.project_identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
    expect(first.status).toBe("completed");
    expect(Array.isArray(first.tool_calls)).toBe(true);
  });

  it("answers an empty page for an install with no runs", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const slug = schedulerHandle();
    const definition = await createScheduler(client, CONTRACT_WORKSPACE, {
      name: `Contract ${slug}`,
      slug,
      prompt: "Contract base prompt.",
    });
    const installed = await createSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, {
      scheduler: definition.id,
      dtstart: new Date().toISOString(),
    });
    try {
      const page = await listBindingRuns(client, CONTRACT_WORKSPACE, projectId, installed.id);
      expect(page.total_count).toBe(0);
      expect(page.results).toEqual([]);
    } finally {
      await deleteSchedulerBinding(client, CONTRACT_WORKSPACE, projectId, installed.id).catch(() => 0);
      await deleteScheduler(client, CONTRACT_WORKSPACE, definition.id).catch(() => 0);
    }
  });
});

describe.skipIf(!contractEnabled)("scheduler occurrences contract", () => {
  it("reads scheduled and past occurrences in a window", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const bindingId = await seededBindingId(client, projectId);
    const from = new Date(Date.now() - 2 * 24 * 60 * 60 * 1000).toISOString();
    const to = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString();
    const window = await listSchedulerOccurrences(client, CONTRACT_WORKSPACE, projectId, { from, to });
    expect(window.has_more).toBe(false);
    expect(window.next_window_start).toBeNull();
    const mine = window.occurrences.filter((row) => row.binding_id === bindingId);
    expect(mine.length).toBeGreaterThan(0);
    expect(mine.some((row) => row.kind === "scheduled")).toBe(true);
    const past = mine.find((row) => row.kind === "past");
    expect(past).toBeDefined();
    expect(past?.agent_run_id).not.toBeNull();
    expect(past?.status).toBe("completed");
  });

  it("rejects a window wider than 90 days with 400", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const from = new Date(Date.now() - 100 * 24 * 60 * 60 * 1000).toISOString();
    const to = new Date().toISOString();
    const wide = await listSchedulerOccurrences(client, CONTRACT_WORKSPACE, projectId, { from, to }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(wide).toBeInstanceOf(ApiError);
    expect((wide as ApiError).status).toBe(400);
  });
});

describe.skipIf(!contractEnabled)("project automations contract", () => {
  it("patches the archive delay, close delay and cancelled-state target", async () => {
    const client = await signedIn();
    const suffix = `${Date.now().toString(36).toUpperCase().slice(-6)}${schedulerNonce + 1}`;
    schedulerNonce += 1;
    const created = await createProject(client, CONTRACT_WORKSPACE, {
      name: `Contract Automations ${suffix}`,
      identifier: `CTAU${suffix}`,
    });
    try {
      const states = await listStates(client, CONTRACT_WORKSPACE, created.id);
      const cancelled = states.find((state) => state.group === "cancelled");
      expect(cancelled).toBeDefined();
      if (!cancelled) return;
      const updated = await updateProject(client, CONTRACT_WORKSPACE, created.id, {
        archive_in: 1,
        close_in: 2,
        default_state: cancelled.id,
      });
      expect(updated.archive_in).toBe(1);
      expect(updated.close_in).toBe(2);
      expect(updated.default_state).toBe(cancelled.id);
      const restored = await updateProject(client, CONTRACT_WORKSPACE, created.id, {
        archive_in: 0,
        close_in: 0,
        default_state: null,
      });
      expect(restored.archive_in).toBe(0);
      expect(restored.close_in).toBe(0);
      expect(restored.default_state).toBeNull();
    } finally {
      await deleteProject(client, CONTRACT_WORKSPACE, created.id).catch(() => 0);
    }
  });

  it("refuses a foreign default state with 400", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const foreign = await updateProject(client, CONTRACT_WORKSPACE, projectId, {
      default_state: "00000000-0000-0000-0000-000000000000",
    }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(foreign).toBeInstanceOf(ApiError);
    expect((foreign as ApiError).status).toBe(400);
  });
});
