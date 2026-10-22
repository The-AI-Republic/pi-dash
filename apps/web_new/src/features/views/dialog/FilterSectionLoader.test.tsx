// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const loader = vi.hoisted(() => ({ loadRichFilterBuilder: vi.fn() }));
vi.mock("../../issues/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../issues/index.js")>()),
  loadRichFilterBuilder: loader.loadRichFilterBuilder,
}));

import { FieldConfigRegistry, type IssuesEntity, type RichFilterBuilderModule } from "../../issues/index.js";
// Test-only deep type import (checks skip test files).
import type { RichFilterBuilderParts } from "../../issues/filters/rich/RichFilterBuilder.js";
import { FilterSectionLoader } from "./FilterSectionLoader.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ENTITY: IssuesEntity = { scope: "project", projectId: PROJECT };

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void; reject: (error: unknown) => void } {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, refuse) => {
    resolve = accept;
    reject = refuse;
  });
  return { promise, resolve, reject };
}

afterEach(() => {
  loader.loadRichFilterBuilder.mockReset();
});

describe("FilterSectionLoader", () => {
  it("shows a loading note while the chunk loads, then the builder", async () => {
    const gate = deferred<RichFilterBuilderModule>();
    const useRichFilterBuilder = vi.fn(
      (): RichFilterBuilderParts => ({
        control: <span data-testid="fake-filter-control" />,
        row: <span data-testid="fake-filter-row" />,
        instanceId: "TEMP-9",
      })
    );
    loader.loadRichFilterBuilder.mockReturnValue(gate.promise);
    const registry = new FieldConfigRegistry();
    render(
      <FilterSectionLoader
        workspaceSlug="acme"
        projectId={PROJECT}
        entity={ENTITY}
        hideCycle={false}
        hideModule={false}
        registry={registry}
      />
    );
    expect(screen.getByText("Loading filters…")).toBeInTheDocument();
    expect(loader.loadRichFilterBuilder).toHaveBeenCalledTimes(1);
    gate.resolve({ useRichFilterBuilder, serializeRichExpression: () => ({}) });
    expect(await screen.findByTestId("fake-filter-control")).toBeInTheDocument();
    expect(screen.getByTestId("fake-filter-row")).toBeInTheDocument();
    expect(useRichFilterBuilder).toHaveBeenCalledWith({
      workspaceSlug: "acme",
      projectId: PROJECT,
      entity: ENTITY,
      hideCycle: false,
      hideModule: false,
      temporary: true,
      temporaryRegistry: registry,
    });
  });

  it("shows an error note when the chunk fails", async () => {
    loader.loadRichFilterBuilder.mockRejectedValue(new Error("chunk gone"));
    render(
      <FilterSectionLoader
        workspaceSlug="acme"
        projectId={PROJECT}
        entity={ENTITY}
        hideCycle={false}
        hideModule={false}
        registry={new FieldConfigRegistry()}
      />
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("Filters could not be loaded.");
  });

  it("ignores a settled load after unmount", async () => {
    const resolveGate = deferred<RichFilterBuilderModule>();
    loader.loadRichFilterBuilder.mockReturnValueOnce(resolveGate.promise);
    const first = render(
      <FilterSectionLoader
        workspaceSlug="acme"
        projectId={PROJECT}
        entity={ENTITY}
        hideCycle={false}
        hideModule={false}
        registry={new FieldConfigRegistry()}
      />
    );
    first.unmount();
    const useRichFilterBuilder = vi.fn(
      (): RichFilterBuilderParts => ({
        control: <span data-testid="late-filter-control" />,
        row: null,
        instanceId: "TEMP-9",
      })
    );
    resolveGate.resolve({ useRichFilterBuilder, serializeRichExpression: () => ({}) });
    await resolveGate.promise;
    await new Promise((done) => setTimeout(done, 0));
    expect(useRichFilterBuilder).not.toHaveBeenCalled();
    expect(screen.queryByTestId("late-filter-control")).not.toBeInTheDocument();

    const rejectGate = deferred<RichFilterBuilderModule>();
    loader.loadRichFilterBuilder.mockReturnValueOnce(rejectGate.promise);
    const second = render(
      <FilterSectionLoader
        workspaceSlug="acme"
        projectId={PROJECT}
        entity={ENTITY}
        hideCycle={false}
        hideModule={false}
        registry={new FieldConfigRegistry()}
      />
    );
    second.unmount();
    rejectGate.reject(new Error("chunk gone"));
    await expect(rejectGate.promise).rejects.toThrow("chunk gone");
    await new Promise((done) => setTimeout(done, 0));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
