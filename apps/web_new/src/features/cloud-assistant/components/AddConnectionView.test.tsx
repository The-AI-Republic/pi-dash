// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Key form (CLOUD-033): save gating per field, the base field per provider,
// the trimmed submit, and the server's refusal staying in the form.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { AddConnectionView } from "./AddConnectionView.js";

function setup(overrides: { saving?: boolean; error?: string | null } = {}) {
  const onSave = vi.fn();
  render(<AddConnectionView saving={overrides.saving ?? false} error={overrides.error ?? null} onSave={onSave} />);
  return { onSave };
}

describe("AddConnectionView", () => {
  it("offers both providers, starts compatible, and blocks save", () => {
    setup();
    const provider = screen.getByRole("combobox", { name: "Provider" });
    expect(provider).toHaveValue("openai_compatible");
    expect(screen.getByRole("option", { name: "OpenAI-compatible" })).toHaveValue("openai_compatible");
    expect(screen.getByRole("option", { name: "Anthropic" })).toHaveValue("anthropic");
    expect(screen.getByRole("textbox", { name: "Base URL" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Model" })).toHaveAttribute("list", "assistant-known-byok-models");
    expect(screen.getByRole("button", { name: "Add connection" })).toBeDisabled();
  });

  it("enables save only when base, model and secret all exist", async () => {
    const user = userEvent.setup();
    setup();
    await user.type(screen.getByRole("combobox", { name: "Model" }), "m");
    await user.type(screen.getByLabelText("API key"), "k");
    expect(screen.getByRole("button", { name: "Add connection" })).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "Base URL" }), "https://openrouter.ai/api/v1");
    expect(screen.getByRole("button", { name: "Add connection" })).toBeEnabled();
  });

  it("hides the base field for the single-vendor provider and enables save without it", async () => {
    const user = userEvent.setup();
    setup();
    await user.type(screen.getByRole("combobox", { name: "Model" }), "claude");
    await user.type(screen.getByLabelText("API key"), "k");
    await user.selectOptions(screen.getByRole("combobox", { name: "Provider" }), "anthropic");
    expect(screen.queryByRole("textbox", { name: "Base URL" })).toBeNull();
    expect(screen.getByRole("button", { name: "Add connection" })).toBeEnabled();
  });

  it("submits the trimmed draft", async () => {
    const user = userEvent.setup();
    const { onSave } = setup();
    await user.type(screen.getByRole("textbox", { name: "Base URL" }), "https://openrouter.ai/api/v1");
    await user.type(screen.getByRole("combobox", { name: "Model" }), "  meta-llama/llama-3.3-70b-instruct  ");
    await user.type(screen.getByLabelText("API key"), "  secret  ");
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    expect(onSave).toHaveBeenCalledWith({
      provider: "openai_compatible",
      baseUrl: "https://openrouter.ai/api/v1",
      modelName: "  meta-llama/llama-3.3-70b-instruct  ",
      apiKey: "  secret  ",
    });
  });

  it("keeps save disabled and submits nothing while a field is missing", async () => {
    const user = userEvent.setup();
    const { onSave } = setup();
    await user.type(screen.getByRole("combobox", { name: "Model" }), "model");
    const save = screen.getByRole("button", { name: "Add connection" });
    expect(save).toBeDisabled();
    await user.click(save);
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "Base URL" })).toHaveValue("");
    expect(screen.getByLabelText("API key")).toHaveValue("");
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

  it("shows progress and blocks a second submit while saving", () => {
    setup({ saving: true });
    expect(screen.getByRole("button", { name: "Adding…" })).toBeDisabled();
  });
});
