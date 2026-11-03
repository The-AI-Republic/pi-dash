// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { exportRequestBody } from "../exportForm.js";
import { exportsKeys } from "./keys.js";
import {
  EXPORT_FAILED_MESSAGE,
  EXPORT_FAILED_TITLE,
  EXPORT_SUCCESS_TITLE,
  selectExportSuccessMessage,
  useCreateWorkspaceExport,
  type CreateWorkspaceExportVariables,
} from "./mutations.js";

interface CapturedCall {
  url: string;
  method: string;
  body?: string | undefined;
}

function stubClient(postStatus = 200) {
  const calls: CapturedCall[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method: string; body?: string }) => {
    calls.push({ url, method: init.method, body: init.body });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.endsWith("/export-issues/")) {
      return {
        status: postStatus,
        url,
        headers: { get: () => "application/json" },
        text: async () =>
          postStatus === 200 ? JSON.stringify({ message: "queued" }) : JSON.stringify({ error: "boom" }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return calls;
}

const VARIABLES: CreateWorkspaceExportVariables = {
  workspaceSlug: "acme",
  value: { provider: "json", projectIds: ["a", "b"] },
};

function Probe({ variables }: { variables: CreateWorkspaceExportVariables }): React.ReactElement {
  const create = useCreateWorkspaceExport();
  const [outcome, setOutcome] = React.useState("idle");
  return (
    <>
      <p data-testid="outcome">{outcome}</p>
      <button type="button" onClick={() => create.mutate(variables)}>
        export
      </button>
      <button
        type="button"
        onClick={() => {
          void create.mutateAsync(variables).then(
            () => setOutcome("ok"),
            () => setOutcome("failed")
          );
        }}
      >
        export async
      </button>
    </>
  );
}

function renderProbe() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  queryClient.setQueryData(exportsKeys.history("acme"), { results: [] });
  render(
    <QueryClientProvider client={queryClient}>
      <Probe variables={VARIABLES} />
    </QueryClientProvider>
  );
  return queryClient;
}

function postedBody(calls: CapturedCall[]): unknown {
  const post = calls.find((call) => call.method === "POST" && call.url.endsWith("/export-issues/"));
  return JSON.parse(post?.body ?? "{}");
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("useCreateWorkspaceExport", () => {
  it("names the chosen format in the success note", () => {
    expect(selectExportSuccessMessage("csv")).toContain("CSV");
    expect(selectExportSuccessMessage("xlsx")).toContain("Excel");
    expect(selectExportSuccessMessage("json")).toContain("JSON");
  });

  it("posts provider, projects and the multi flag, then toasts and re-reads the history", async () => {
    const calls = stubClient();
    const queryClient = renderProbe();

    fireEvent.click(screen.getByRole("button", { name: "export" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledTimes(1));
    expect(notifySuccess).toHaveBeenCalledWith(EXPORT_SUCCESS_TITLE, expect.stringContaining("JSON"));
    expect(EXPORT_SUCCESS_TITLE).toBe("Export successful");

    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toBe("https://api.test/api/workspaces/acme/export-issues/");
    expect(postedBody(calls)).toEqual(exportRequestBody(VARIABLES.value));
    expect(postedBody(calls)).toEqual({ provider: "json", project: ["a", "b"], multiple: true });
    expect(Object.keys(postedBody(calls) as Record<string, unknown>).sort()).toEqual([
      "multiple",
      "project",
      "provider",
    ]);
    await waitFor(() => expect(queryClient.getQueryState(exportsKeys.history("acme"))?.isInvalidated).toBe(true));
  });

  it("sends one project without the multi flag", async () => {
    const calls = stubClient();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <Probe variables={{ workspaceSlug: "acme", value: { provider: "csv", projectIds: ["a"] } }} />
      </QueryClientProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "export" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledTimes(1));
    expect(notifySuccess).toHaveBeenCalledWith(EXPORT_SUCCESS_TITLE, expect.stringContaining("CSV"));
    expect(postedBody(calls)).toEqual({ provider: "csv", project: ["a"], multiple: false });
  });

  it("toasts a retry prompt on failure and leaves the history alone", async () => {
    const calls = stubClient(500);
    const queryClient = renderProbe();

    fireEvent.click(screen.getByRole("button", { name: "export async" }));
    await waitFor(() => expect(screen.getByTestId("outcome")).toHaveTextContent("failed"));
    expect(notifyError).toHaveBeenCalledWith(EXPORT_FAILED_TITLE, EXPORT_FAILED_MESSAGE);
    expect(EXPORT_FAILED_TITLE).toBe("Error");
    expect(EXPORT_FAILED_MESSAGE).toBe("Export was unsuccessful. Please try again.");
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(calls.filter((call) => call.method === "POST")).toHaveLength(1);
    expect(queryClient.getQueryState(exportsKeys.history("acme"))?.isInvalidated).toBe(false);
  });
});
