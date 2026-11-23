// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Language-model form (parity/specs/profile.md §10, PROF-021): the provider
// switch hides the endpoint for a named provider, every control is
// labelled, a stored key only shows its placeholder, and the model-ID
// editor (NEWPIDASH-394) lists each ID with its status, tests one ID,
// and confirms removing the last one. Save and remove gate on what the
// server already holds plus what was typed.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { LlmConfig } from "@pidash/api-client";

import {
  LLM_BASE_URL_PLACEHOLDER,
  LLM_KEY_PLACEHOLDER_EMPTY,
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
    failures: {},
    pendingIds: [],
    saveAnyway: false,
    onChange: vi.fn(),
    onSave: vi.fn(),
    onTestModel: vi.fn(),
    onRemoveModel: vi.fn(),
    onRemoveKey: vi.fn(),
    saving: false,
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
      models: ["model-a"],
      key: "",
    });
    expect(providerFormFrom(config({ models: ["m-1", "m-2"] })).models).toEqual(["m-1", "m-2"]);
    const values = providerFormFrom(config());
    renderForm(values, { values });
    expect(screen.getByRole("heading", { name: LLM_SECTION_HEADING })).toBeInTheDocument();
    expect(screen.getByLabelText("Provider")).toHaveValue("openai_compatible");
    expect(screen.getByLabelText("Base URL")).toHaveValue("https://llm.test/v1");
    expect(screen.getByText("model-a")).toBeInTheDocument();
    expect(screen.getByLabelText("API key")).toHaveValue("");
    expect(screen.getByPlaceholderText(LLM_KEY_PLACEHOLDER_EMPTY)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(LLM_BASE_URL_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.queryByLabelText("Model")).toBeNull();
  });

  it("hides the endpoint for the named provider and reports the switch", async () => {
    const user = userEvent.setup();
    function Harness(): React.ReactElement {
      const [values, setValues] = React.useState<ProviderFormValues>({
        provider: "openai_compatible",
        baseUrl: "https://x.test",
        models: ["m"],
        key: "",
      });
      return (
        <ProviderForm
          config={config()}
          values={values}
          failures={{}}
          pendingIds={[]}
          saveAnyway={false}
          onChange={setValues}
          onSave={vi.fn()}
          onTestModel={vi.fn()}
          onRemoveModel={vi.fn()}
          onRemoveKey={vi.fn()}
          saving={false}
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

  it("reports each typed field and grows the model-ID list", async () => {
    const user = userEvent.setup();
    const props = renderForm(emptyProviderForm());
    await user.type(screen.getByLabelText("Base URL"), "u");
    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ baseUrl: "u" }));
    await user.type(screen.getByLabelText("Add a model ID this key serves"), "g{Enter}");
    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ models: ["g"] }));
    await user.type(screen.getByLabelText("API key"), "k");
    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({ key: "k" }));
  });

  it("marks each stored ID by its verification stamp", () => {
    const stored = config({ models: ["model-a", "model-b"], model_verified_at: { "model-a": "2026-10-19T12:00:00Z" } });
    renderForm(providerFormFrom(stored), { config: stored });
    const rows = screen.getAllByTestId("byok-model-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]!).getByTestId("byok-model-status")).toHaveTextContent("Verified");
    expect(within(rows[1]!).getByTestId("byok-model-status")).toHaveTextContent("Not verified");
  });

  it("shows a refused save's message beside its ID and offers Save anyway", () => {
    renderForm(
      { provider: "openai_compatible", baseUrl: "", models: ["model-a", "model-b"], key: "" },
      {
        failures: { "model-b": "Model not accepted by the provider." },
        failedIds: ["model-b"],
        saveAnyway: true,
      }
    );
    expect(screen.getByTestId("byok-model-detail")).toHaveTextContent("Model not accepted by the provider.");
    const rows = screen.getAllByTestId("byok-model-row");
    expect(within(rows[1]!).getByTestId("byok-model-status")).toHaveTextContent("Failed");
    expect(screen.getByRole("button", { name: "Save anyway" })).toBeInTheDocument();
  });

  it("marks a refused-but-unverified ID as not verified", () => {
    renderForm(
      { provider: "openai_compatible", baseUrl: "", models: ["model-a"], key: "" },
      {
        failures: { "model-a": "Timed out." },
        failedIds: [],
        saveAnyway: true,
      }
    );
    expect(screen.getByTestId("byok-model-status")).toHaveTextContent("Not verified");
  });

  it("keeps save live over a stored key and offers remove only for one", () => {
    renderForm(
      { provider: "openai_compatible", baseUrl: "", models: ["m"], key: "" },
      { config: config({ has_api_key: true }) }
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Test m" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
    expect(screen.getByLabelText("API key")).toHaveAttribute("placeholder", SAVED_KEY_PLACEHOLDER);
  });

  it("refuses a first save without a model ID or a typed key", () => {
    renderForm({ provider: "openai_compatible", baseUrl: "u", models: [], key: "k" });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    renderForm({ provider: "openai_compatible", baseUrl: "u", models: ["m"], key: "  " });
    expect(screen.getAllByRole("button", { name: "Save" })[1]).toBeDisabled();
  });

  it("hides remove and the Test buttons while nothing is stored", () => {
    renderForm({ provider: "openai_compatible", baseUrl: "u", models: ["m"], key: "k" });
    expect(screen.queryByRole("button", { name: "Remove" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Test m" })).toBeNull();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("fires save, per-ID test, model remove and key remove", async () => {
    const user = userEvent.setup();
    const props = renderForm(
      { provider: "openai_compatible", baseUrl: "u", models: ["m", "n"], key: "k" },
      { config: config({ has_api_key: true }) }
    );
    await user.click(screen.getByRole("button", { name: "Save" }));
    await user.click(screen.getByRole("button", { name: "Test m" }));
    await user.click(screen.getByRole("button", { name: "Remove n" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(props.onSave).toHaveBeenCalledTimes(1);
    expect(props.onSave).toHaveBeenCalledWith(false);
    expect(props.onTestModel).toHaveBeenCalledWith("m");
    expect(props.onRemoveModel).toHaveBeenCalledWith("n");
    expect(props.onRemoveKey).toHaveBeenCalledTimes(1);
  });

  it("confirms removing the last model ID before the caller deletes the key", async () => {
    const user = userEvent.setup();
    const props = renderForm(
      { provider: "openai_compatible", baseUrl: "u", models: ["m"], key: "" },
      { config: config({ has_api_key: true }) }
    );
    await user.click(screen.getByRole("button", { name: "Remove m" }));
    expect(props.onRemoveModel).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Remove both" }));
    expect(props.onRemoveModel).toHaveBeenCalledWith("m");
  });

  it("marks in-flight controls busy", () => {
    renderForm(
      { provider: "openai_compatible", baseUrl: "u", models: ["m"], key: "k" },
      { config: config({ has_api_key: true }), saving: true, removing: true }
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
  });
});
