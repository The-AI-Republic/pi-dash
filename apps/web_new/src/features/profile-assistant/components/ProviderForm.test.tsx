// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Language-model form (parity/specs/profile.md §10, PROF-021): the provider
// switch hides the endpoint for a named provider, every control is
// labelled, a stored key only shows its placeholder, and save/test/remove
// gate on what the server already holds plus what was typed.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { LlmConfig } from "@pidash/api-client";

import {
  LLM_BASE_URL_PLACEHOLDER,
  LLM_KEY_PLACEHOLDER_EMPTY,
  LLM_MODEL_PLACEHOLDER,
  LLM_SECTION_HEADING,
  SAVED_KEY_PLACEHOLDER,
} from "../lib/copy.js";
import { ProviderForm, emptyProviderForm, providerFormFrom, type ProviderFormValues } from "./ProviderForm.js";

function config(overrides: Partial<LlmConfig> = {}): LlmConfig {
  return {
    provider_kind: "openai_compatible",
    base_url: "https://llm.test/v1",
    model_name: "model-a",
    has_api_key: false,
    last_verified_at: null,
    ...overrides,
  };
}

function renderForm(values: ProviderFormValues, overrides: Partial<React.ComponentProps<typeof ProviderForm>> = {}) {
  const props: React.ComponentProps<typeof ProviderForm> = {
    config: config(),
    values,
    onChange: vi.fn(),
    onSave: vi.fn(),
    onTest: vi.fn(),
    onRemove: vi.fn(),
    saving: false,
    testing: false,
    removing: false,
    ...overrides,
  };
  render(<ProviderForm {...props} />);
  return props;
}

describe("ProviderForm", () => {
  it("seeds from a stored config and starts with a blank key", () => {
    expect(providerFormFrom(undefined)).toEqual(emptyProviderForm());
    expect(providerFormFrom(config({ provider_kind: "anthropic", base_url: "" }))).toEqual({
      provider: "anthropic",
      baseUrl: "",
      model: "model-a",
      key: "",
    });
    const values = providerFormFrom(config());
    renderForm(values, { values });
    expect(screen.getByRole("heading", { name: LLM_SECTION_HEADING })).toBeInTheDocument();
    expect(screen.getByLabelText("Provider")).toHaveValue("openai_compatible");
    expect(screen.getByLabelText("Base URL")).toHaveValue("https://llm.test/v1");
    expect(screen.getByLabelText("Model")).toHaveValue("model-a");
    expect(screen.getByLabelText("API key")).toHaveValue("");
    expect(screen.getByPlaceholderText(LLM_KEY_PLACEHOLDER_EMPTY)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(LLM_BASE_URL_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(LLM_MODEL_PLACEHOLDER)).toBeInTheDocument();
  });

  it("hides the endpoint for the named provider and reports the switch", async () => {
    const user = userEvent.setup();
    function Harness(): React.ReactElement {
      const [values, setValues] = React.useState<ProviderFormValues>({
        provider: "openai_compatible",
        baseUrl: "https://x.test",
        model: "m",
        key: "",
      });
      return (
        <ProviderForm
          config={config()}
          values={values}
          onChange={setValues}
          onSave={vi.fn()}
          onTest={vi.fn()}
          onRemove={vi.fn()}
          saving={false}
          testing={false}
          removing={false}
        />
      );
    }
    render(<Harness />);
    expect(screen.getByLabelText("Base URL")).toHaveValue("https://x.test");
    await user.selectOptions(screen.getByLabelText("Provider"), "anthropic");
    expect(screen.queryByLabelText("Base URL")).not.toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("Provider"), "openai_compatible");
    expect(screen.getByLabelText("Base URL")).toHaveValue("https://x.test");
  });

  it("reports each typed field", async () => {
    const user = userEvent.setup();
    const props = renderForm(emptyProviderForm());
    await user.type(screen.getByLabelText("Base URL"), "u");
    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ baseUrl: "u" }));
    await user.type(screen.getByLabelText("Model"), "g");
    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ model: "g" }));
    await user.type(screen.getByLabelText("API key"), "k");
    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ key: "k" }));
  });

  it("keeps save live over a stored key and offers remove only for one", () => {
    renderForm(
      { provider: "openai_compatible", baseUrl: "", model: "m", key: "" },
      { config: config({ has_api_key: true }) }
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
    expect(screen.getByLabelText("API key")).toHaveAttribute("placeholder", SAVED_KEY_PLACEHOLDER);
  });

  it("refuses a first save without a typed key", () => {
    renderForm({ provider: "openai_compatible", baseUrl: "u", model: "m", key: "  " });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("hides remove and disables test while nothing is stored", () => {
    renderForm({ provider: "openai_compatible", baseUrl: "u", model: "m", key: "k" });
    expect(screen.queryByRole("button", { name: "Remove" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("fires save, test and remove", async () => {
    const user = userEvent.setup();
    const props = renderForm(
      { provider: "openai_compatible", baseUrl: "u", model: "m", key: "k" },
      { config: config({ has_api_key: true }) }
    );
    await user.click(screen.getByRole("button", { name: "Save" }));
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(props.onSave).toHaveBeenCalledTimes(1);
    expect(props.onTest).toHaveBeenCalledTimes(1);
    expect(props.onRemove).toHaveBeenCalledTimes(1);
  });

  it("shows the verification stamp once verified", () => {
    renderForm(emptyProviderForm(), { config: config({ last_verified_at: "2026-10-19T12:00:00Z" }) });
    expect(screen.getByText(/Last verified:/)).toBeInTheDocument();
  });

  it("marks in-flight controls busy", () => {
    renderForm(
      { provider: "openai_compatible", baseUrl: "u", model: "m", key: "k" },
      { config: config({ has_api_key: true }), saving: true, testing: true, removing: true }
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
  });
});
