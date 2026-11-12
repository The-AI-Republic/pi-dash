// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modal work-item type picker (NEWFRONT-984): options come from the
// registered provider, the trigger names the pick, "No type" clears
// back to the project default, and a failed load reports in place.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { IssueTypePicker } from "./IssueTypePicker.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return React.createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  registerEditionIssuesProviders({
    workItemTypes: async () => [
      { id: "t-bug", name: "Bug" },
      { id: "t-epic", name: "Epic" },
    ],
  });
  return { queryClient };
}

afterEach(() => {
  cleanup();
  resetEditionIssuesProviders();
});

describe("IssueTypePicker", () => {
  it("lists the project types and picks one", async () => {
    const user = userEvent.setup();
    const { queryClient } = setup();
    const onTypeChange = vi.fn();
    render(
      <IssueTypePicker
        workspaceSlug="acme"
        projectId="p-1"
        disabled={false}
        onTemplateApply={() => undefined}
        propertyValues={{}}
        onPropertyValuesChange={() => undefined}
        typeId={null}
        onTypeChange={onTypeChange}
      />,
      { wrapper: wrapper(queryClient) }
    );
    await user.click(screen.getByRole("button", { name: "No type" }));
    await user.click(await screen.findByRole("option", { name: "Bug" }));
    expect(onTypeChange).toHaveBeenCalledWith("t-bug");
  });

  it("names the current pick and clears back to the default", async () => {
    const user = userEvent.setup();
    const { queryClient } = setup();
    const onTypeChange = vi.fn();
    render(
      <IssueTypePicker
        workspaceSlug="acme"
        projectId="p-1"
        disabled={false}
        onTemplateApply={() => undefined}
        propertyValues={{}}
        onPropertyValuesChange={() => undefined}
        typeId="t-epic"
        onTypeChange={onTypeChange}
      />,
      { wrapper: wrapper(queryClient) }
    );
    await user.click(await screen.findByRole("button", { name: "Epic" }));
    await user.click(await screen.findByRole("option", { name: "No type" }));
    expect(onTypeChange).toHaveBeenCalledWith(null);
  });

  it("offers only the default when the project has no types", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    registerEditionIssuesProviders({ workItemTypes: async () => [] });
    const onTypeChange = vi.fn();
    render(
      <IssueTypePicker
        workspaceSlug="acme"
        projectId="p-1"
        disabled={false}
        onTemplateApply={() => undefined}
        propertyValues={{}}
        onPropertyValuesChange={() => undefined}
        typeId={null}
        onTypeChange={onTypeChange}
      />,
      { wrapper: wrapper(queryClient) }
    );
    await user.click(screen.getByRole("button", { name: "No type" }));
    expect(await screen.findByRole("option", { name: "No type" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Bug" })).not.toBeInTheDocument();
  });

  it("reports a failed type load in place", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    registerEditionIssuesProviders({
      workItemTypes: async () => {
        throw new Error("types down");
      },
    });
    render(
      <IssueTypePicker
        workspaceSlug="acme"
        projectId="p-1"
        disabled={false}
        onTemplateApply={() => undefined}
        propertyValues={{}}
        onPropertyValuesChange={() => undefined}
        typeId={null}
        onTypeChange={() => undefined}
      />,
      { wrapper: wrapper(queryClient) }
    );
    await user.click(screen.getByRole("button", { name: "No type" }));
    expect(await screen.findByText("Issue types failed to load.")).toBeInTheDocument();
  });
});
