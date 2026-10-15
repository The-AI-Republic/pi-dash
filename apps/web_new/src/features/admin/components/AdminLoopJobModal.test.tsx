// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job create/edit modal: blank-with-defaults creates, pre-filled
// edits with the builtin slug locked, the save gate, the eight-key
// save bodies, and server-text failure notices that keep the modal
// open. Writes run through the real app client against a stub
// transport so the POST/PATCH bodies are pinned here.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LoopJob, Transport } from "@pidash/api-client";
import { ToastHost } from "@pidash/kit";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager } from "../../../shared/shell/index.js";
import {
  LOOP_JOB_DEFAULT_RRULE,
  LOOP_JOB_FIELD_LABELS,
  LOOP_JOB_MODAL_CREATE_TITLE,
  LOOP_JOB_MODAL_EDIT_TITLE,
} from "../loopJobModal.js";
import { AdminLoopJobModal } from "./AdminLoopJobModal.js";

function storedJob(overrides: Partial<LoopJob> = {}): LoopJob {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    slug: "probe-job",
    name: "Probe job",
    public_name: "Probe public",
    public_description: "Probe description",
    prompt: "Probe prompt",
    min_role: 20,
    enabled: true,
    is_builtin: false,
    dtstart: null,
    rrule: "FREQ=WEEKLY;BYDAY=MO",
    tzid: "Europe/Paris",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

interface PatchCall {
  id: string;
  body: unknown;
}

interface ModalHarness {
  posts: unknown[];
  patches: PatchCall[];
  onClose: ReturnType<typeof vi.fn>;
}

async function renderModal(options: {
  job?: LoopJob;
  saveGate?: Promise<void>;
  onPost?: (body: unknown) => Promise<{ status: number; body: unknown }>;
  onPatch?: (id: string, body: unknown) => Promise<{ status: number; body: unknown }>;
}): Promise<ModalHarness> {
  const posts: unknown[] = [];
  const patches: PatchCall[] = [];
  const onClose = vi.fn();
  const saveGate = options.saveGate ?? Promise.resolve();
  const onPost =
    options.onPost ??
    (async (body: unknown) => ({
      status: 201,
      body: {
        ...(body as Record<string, unknown>),
        id: "44444444-4444-4444-8444-444444444444",
        enabled: true,
        is_builtin: false,
        dtstart: null,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    }));
  const onPatch =
    options.onPatch ??
    (async (id: string, body: unknown) => ({
      status: 200,
      body: { ...storedJob({ id }), ...(body as Record<string, unknown>) },
    }));
  const transport: Transport = async (url, init) => {
    const pathname = new URL(url, "https://api.test").pathname;
    if (init.method === "POST" && pathname === "/api/instances/loop/jobs/") {
      await saveGate;
      const body = init.body === undefined ? undefined : (JSON.parse(init.body) as unknown);
      posts.push(body);
      const result = await onPost(body);
      return {
        status: result.status,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(result.body)),
      };
    }
    const patchMatch = /^\/api\/instances\/loop\/jobs\/([^/]+)\/$/.exec(pathname);
    if (init.method === "PATCH" && patchMatch?.[1] !== undefined) {
      await saveGate;
      const id = patchMatch[1];
      const body = init.body === undefined ? undefined : (JSON.parse(init.body) as unknown);
      patches.push({ id, body });
      const result = await onPatch(id, body);
      return {
        status: result.status,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(result.body)),
      };
    }
    return {
      status: 404,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
    };
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ToastHost manager={getToastManager()} />
      <AdminLoopJobModal job={options.job} onClose={onClose} />
    </QueryClientProvider>
  );
  await screen.findByTestId("loop-job-modal-form");
  return { posts, patches, onClose };
}

afterEach(() => {
  resetAppClient();
});

describe("AdminLoopJobModal create", () => {
  it("opens blank with member, UTC, and daily defaults and a disabled save (ADM-055)", async () => {
    await renderModal({});
    expect(LOOP_JOB_MODAL_CREATE_TITLE).toBe("New job");
    expect(screen.getByRole("heading", { name: "New job" })).toBeInTheDocument();
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug)).toHaveValue("");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name)).toHaveValue("");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicName)).toHaveValue("");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicDescription)).toHaveValue("");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt)).toHaveValue("");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.minRole)).toHaveValue("15");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.tzid)).toHaveValue("UTC");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.rrule)).toHaveValue(LOOP_JOB_DEFAULT_RRULE);
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug)).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
  });

  it("scrolls the body within a capped height (ADM-057)", async () => {
    await renderModal({});
    const body = screen.getByTestId("loop-job-modal-body");
    expect(body.className).toContain("overflow-y-auto");
    expect(body.className).toMatch(/max-h-\[/);
  });

  it("enables save once slug, both names, and prompt are non-blank (ADM-057)", async () => {
    const user = userEvent.setup();
    await renderModal({});
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug), "probe-job");
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name), "Probe job");
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicName), "Probe public");
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt), "Probe prompt");
    expect(save).toBeEnabled();
    await user.clear(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt));
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt), "   ");
    expect(save).toBeDisabled();
  });

  it("saves the eight-key body and closes with no notice (ADM-055)", async () => {
    const user = userEvent.setup();
    const harness = await renderModal({});
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug), "probe-job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name), "Probe job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicName), "Probe public");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicDescription), "Probe description");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt), "Probe prompt");
    await user.selectOptions(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.minRole), "20");
    await user.clear(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.tzid));
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.tzid), "Europe/Paris");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(harness.onClose).toHaveBeenCalledTimes(1));
    expect(harness.posts).toEqual([
      {
        slug: "probe-job",
        name: "Probe job",
        public_name: "Probe public",
        public_description: "Probe description",
        prompt: "Probe prompt",
        min_role: 20,
        rrule: LOOP_JOB_DEFAULT_RRULE,
        tzid: "Europe/Paris",
      },
    ]);
    expect(harness.patches).toEqual([]);
    await waitFor(() => expect(screen.queryByTestId("toast-notification")).not.toBeInTheDocument());
  });

  it("keeps save disabled until the create lands", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const saveGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const harness = await renderModal({ saveGate });
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug), "probe-job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name), "Probe job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicName), "Probe public");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt), "Probe prompt");
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeEnabled();
    await user.click(save);
    await waitFor(() => expect(save).toBeDisabled());
    expect(harness.onClose).not.toHaveBeenCalled();
    release();
    await waitFor(() => expect(harness.onClose).toHaveBeenCalledTimes(1));
  });

  it("ignores a second submit while a save is in flight", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const saveGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const harness = await renderModal({ saveGate });
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug), "probe-job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name), "Probe job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicName), "Probe public");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt), "Probe prompt");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeDisabled());
    // Implicit submissions (Enter in a field) bypass the disabled
    // button, so they must be dropped while saving too.
    fireEvent.submit(screen.getByTestId("loop-job-modal-form"));
    fireEvent.submit(screen.getByTestId("loop-job-modal-form"));
    release();
    await waitFor(() => expect(harness.onClose).toHaveBeenCalledTimes(1));
    expect(harness.posts).toHaveLength(1);
  });

  it("sends nothing when submitted with blank fields", async () => {
    const harness = await renderModal({});
    fireEvent.submit(screen.getByTestId("loop-job-modal-form"));
    expect(harness.posts).toEqual([]);
    expect(harness.patches).toEqual([]);
    expect(harness.onClose).not.toHaveBeenCalled();
  });

  it("cancels without writing (ADM-056)", async () => {
    const user = userEvent.setup();
    const harness = await renderModal({});
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug), "probe-job");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(harness.onClose).toHaveBeenCalledTimes(1);
    expect(harness.posts).toEqual([]);
    expect(harness.patches).toEqual([]);
  });

  it("dismisses through the dialog close control", async () => {
    const user = userEvent.setup();
    const harness = await renderModal({});
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(harness.onClose).toHaveBeenCalledTimes(1);
    expect(harness.posts).toEqual([]);
  });
});

describe("AdminLoopJobModal edit", () => {
  it("prefills every field and enables save (ADM-056)", async () => {
    await renderModal({ job: storedJob() });
    expect(LOOP_JOB_MODAL_EDIT_TITLE).toBe("Edit job");
    expect(screen.getByRole("heading", { name: "Edit job" })).toBeInTheDocument();
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug)).toHaveValue("probe-job");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name)).toHaveValue("Probe job");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicName)).toHaveValue("Probe public");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicDescription)).toHaveValue("Probe description");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt)).toHaveValue("Probe prompt");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.minRole)).toHaveValue("20");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.tzid)).toHaveValue("Europe/Paris");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.rrule)).toHaveValue("FREQ=WEEKLY;BYDAY=MO");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug)).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("locks the slug for builtin jobs (ADM-056)", async () => {
    const user = userEvent.setup();
    const harness = await renderModal({ job: storedJob({ is_builtin: true }) });
    expect(screen.getByRole("heading", { name: "Edit job" })).toBeInTheDocument();
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug)).toHaveValue("probe-job");
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug)).toBeDisabled();
    expect(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name)).not.toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(harness.onClose).toHaveBeenCalledTimes(1);
    expect(harness.posts).toEqual([]);
    expect(harness.patches).toEqual([]);
  });

  it("saves the eight-field patch and closes (ADM-056)", async () => {
    const user = userEvent.setup();
    const harness = await renderModal({ job: storedJob() });
    await user.clear(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name));
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name), "Renamed probe");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(harness.onClose).toHaveBeenCalledTimes(1));
    expect(harness.patches).toEqual([
      {
        id: "33333333-3333-4333-8333-333333333333",
        body: {
          slug: "probe-job",
          name: "Renamed probe",
          public_name: "Probe public",
          public_description: "Probe description",
          prompt: "Probe prompt",
          min_role: 20,
          rrule: "FREQ=WEEKLY;BYDAY=MO",
          tzid: "Europe/Paris",
        },
      },
    ]);
    expect(harness.posts).toEqual([]);
  });

  it("keeps save disabled until the edit lands", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const saveGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const harness = await renderModal({ job: storedJob(), saveGate });
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeEnabled();
    await user.click(save);
    await waitFor(() => expect(save).toBeDisabled());
    expect(harness.onClose).not.toHaveBeenCalled();
    release();
    await waitFor(() => expect(harness.onClose).toHaveBeenCalledTimes(1));
  });

  // Last: the failure notices linger in the shared toast manager, so
  // no later test may assert the absence of toasts.
  it("failed save toasts the server text and stays open (ADM-057)", async () => {
    const user = userEvent.setup();
    const harness = await renderModal({
      onPost: async () => ({ status: 500, body: { error: "parity-probe-failure" } }),
    });
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug), "probe-job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name), "Probe job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicName), "Probe public");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt), "Probe prompt");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Save failed")).toBeInTheDocument();
    expect(screen.getByText("parity-probe-failure")).toBeInTheDocument();
    expect(harness.onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "New job" })).toBeInTheDocument();
  });

  it("invalid recurrence surfaces the server detail and stays open (ADM-057)", async () => {
    const user = userEvent.setup();
    const harness = await renderModal({
      onPost: async () => ({ status: 400, body: { detail: "Enter a valid recurrence rule." } }),
    });
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.slug), "probe-job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.name), "Probe job");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.publicName), "Probe public");
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.prompt), "Probe prompt");
    await user.clear(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.rrule));
    await user.type(screen.getByLabelText(LOOP_JOB_FIELD_LABELS.rrule), "bogus-rrule");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Save failed")).toBeInTheDocument();
    const notice = screen.getByText("Enter a valid recurrence rule.");
    expect(notice).toBeInTheDocument();
    expect(notice.textContent).not.toBe("Check the fields and try again.");
    expect(harness.onClose).not.toHaveBeenCalled();
  });
});
