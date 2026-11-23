// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Key form (CLOUD-033): save gating per field, the base field per provider,
// the multi-model list for a new key (NEWPIDASH-394), the trimmed submit,
// and the server's refusal staying in the form.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ModelTestResults } from "@pidash/api-client";

import { AddConnectionView } from "./AddConnectionView.js";

function setup(overrides: { saving?: boolean; error?: string | null; results?: ModelTestResults | null } = {}) {
  const onSave = vi.fn();
  const view = render(
    <AddConnectionView
      saving={overrides.saving ?? false}
      error={overrides.error ?? null}
      results={overrides.results ?? null}
      onSave={onSave}
    />
  );
  return { onSave, rerender: view.rerender };
}

async function addModel(user: ReturnType<typeof userEvent.setup>, model: string): Promise<void> {
  await user.type(screen.getByLabelText("Add a model ID this key serves"), `${model}{Enter}`);
}

describe("AddConnectionView", () => {
  it("offers both providers, starts compatible, and blocks save", () => {
    setup();
    const provider = screen.getByRole("combobox", { name: "Provider" });
    expect(provider).toHaveValue("openai_compatible");
    expect(screen.getByRole("option", { name: "OpenAI-compatible" })).toHaveValue("openai_compatible");
    expect(screen.getByRole("option", { name: "Anthropic" })).toHaveValue("anthropic");
    expect(screen.getByRole("textbox", { name: "Base URL" })).toBeInTheDocument();
    expect(screen.getByLabelText("Add a model ID this key serves")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add connection" })).toBeDisabled();
  });

  it("enables save only when base, at least one model and secret all exist", async () => {
    const user = userEvent.setup();
    setup();
    await addModel(user, "m");
    await user.type(screen.getByLabelText("API key"), "k");
    expect(screen.getByRole("button", { name: "Add connection" })).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "Base URL" }), "https://openrouter.ai/api/v1");
    expect(screen.getByRole("button", { name: "Add connection" })).toBeEnabled();
  });

  it("hides the base field for the single-vendor provider and enables save without it", async () => {
    const user = userEvent.setup();
    setup();
    await addModel(user, "claude");
    await user.type(screen.getByLabelText("API key"), "k");
    await user.selectOptions(screen.getByRole("combobox", { name: "Provider" }), "anthropic");
    expect(screen.queryByRole("textbox", { name: "Base URL" })).toBeNull();
    expect(screen.getByRole("button", { name: "Add connection" })).toBeEnabled();
  });

  it("submits the draft with its model-ID list", async () => {
    const user = userEvent.setup();
    const { onSave } = setup();
    await user.type(screen.getByRole("textbox", { name: "Base URL" }), "https://openrouter.ai/api/v1");
    await addModel(user, "gpt-6");
    await addModel(user, "gpt-6.2");
    await user.type(screen.getByLabelText("API key"), "  secret  ");
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    expect(onSave).toHaveBeenCalledWith(
      {
        provider: "openai_compatible",
        baseUrl: "https://openrouter.ai/api/v1",
        models: ["gpt-6", "gpt-6.2"],
        apiKey: "  secret  ",
      },
      false
    );
  });

  it("keeps save disabled and submits nothing while a model or the secret is missing", async () => {
    const user = userEvent.setup();
    const { onSave } = setup();
    await user.type(screen.getByLabelText("API key"), "k");
    const save = screen.getByRole("button", { name: "Add connection" });
    expect(save).toBeDisabled();
    await user.click(save);
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "Base URL" })).toHaveValue("");
  });

  it("submits nothing when the form is fired while still incomplete", () => {
    const { onSave } = setup();
    fireEvent.submit(screen.getByRole("form", { name: "Add connection" }));
    expect(onSave).not.toHaveBeenCalled();
  });

  it("keeps the server's refusal in the form", () => {
    setup({ error: "Base URL is unreachable" });
    expect(screen.getByRole("alert")).toHaveTextContent("Could not add connection: Base URL is unreachable");
  });

  it("uses the fallback copy for an empty refusal", () => {
    setup({ error: "" });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not add connection: The server rejected the configuration"
    );
  });

  it("shows per-ID results with a Save-anyway retry for unverified-only refusals", async () => {
    const user = userEvent.setup();
    const { onSave, rerender } = setup();
    await addModel(user, "flaky");
    rerender(
      <AddConnectionView
        saving={false}
        error={null}
        results={{
          models: [{ model: "flaky", status: "unverified", code: "provider_unreachable", detail: "Timed out." }],
        }}
        onSave={onSave}
      />
    );
    expect(screen.getByTestId("byok-model-detail")).toHaveTextContent("Timed out.");
    expect(screen.getByTestId("byok-model-status")).toHaveTextContent("Not verified");
    await user.click(screen.getByRole("button", { name: "Save anyway" }));
    expect(onSave).toHaveBeenCalled();
    expect(onSave.mock.calls[0]?.[1]).toBe(true);
  });

  it("shows progress and blocks a second submit while saving", () => {
    setup({ saving: true });
    expect(screen.getByRole("button", { name: "Adding…" })).toBeDisabled();
  });
});
