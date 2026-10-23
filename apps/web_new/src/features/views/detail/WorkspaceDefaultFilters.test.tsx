// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const loader = vi.hoisted(() => ({ loadRichFilterBuilder: vi.fn() }));
vi.mock("../../issues/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../issues/index.js")>()),
  loadRichFilterBuilder: loader.loadRichFilterBuilder,
}));

import type { RichExpression, RichFilterBuilderModule } from "../../issues/index.js";
// Test-only deep imports (checks skip test files): the registry class
// and snapshot helpers the mocked module carries for the host.
import { buildViewCreate, buildViewPatch, savedViewKeys } from "../../issues/filters/rich/views.js";
import { FieldConfigRegistry as RealRegistry } from "../../issues/filters/rich/temporary.js";
// Test-only deep type import (checks skip test files).
import type { RichFilterBuilderParts } from "../../issues/filters/rich/RichFilterBuilder.js";
import { WorkspaceDefaultFilters } from "./WorkspaceDefaultFilters.js";

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void; reject: (error: unknown) => void } {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, refuse) => {
    resolve = accept;
    reject = refuse;
  });
  return { promise, resolve, reject };
}

function fakeModule(
  useRichFilterBuilder: ReturnType<typeof vi.fn>,
  serializeRichExpression: (rows: readonly unknown[]) => RichExpression = () => ({})
): RichFilterBuilderModule {
  return {
    useRichFilterBuilder,
    serializeRichExpression: serializeRichExpression as RichFilterBuilderModule["serializeRichExpression"],
    parseRichExpression: () => [],
    FieldConfigRegistry: RealRegistry,
    buildViewCreate,
    buildViewPatch,
    viewDisplaySnapshot: () => ({ displayFilters: {}, displayProperties: {} }),
    savedViewKeys,
  };
}

function fakeBuilder(conditions: RichFilterBuilderParts["conditions"] = []): ReturnType<typeof vi.fn> {
  return vi.fn(
    (): RichFilterBuilderParts => ({
      control: <span data-testid="fake-filter-control" />,
      conditions,
      row: <span data-testid="fake-filter-row" />,
      instanceId: "TEMP-9",
      addedThisVisit: false,
      clearFilters: () => {},
      hasCompleteCondition: false,
    })
  );
}

function renderHost(onExpressionChange: (expression: RichExpression) => void = () => {}): void {
  render(
    <WorkspaceDefaultFilters workspaceSlug="acme" onExpressionChange={onExpressionChange}>
      {(slots) => (
        <>
          <div data-testid="control-slot">{slots.control}</div>
          <div data-testid="row-slot">{slots.row}</div>
        </>
      )}
    </WorkspaceDefaultFilters>
  );
}

afterEach(() => {
  loader.loadRichFilterBuilder.mockReset();
});

describe("WorkspaceDefaultFilters", () => {
  it("places nothing in the slots while the chunk loads, then the builder's parts", async () => {
    const gate = deferred<RichFilterBuilderModule>();
    const useRichFilterBuilder = fakeBuilder();
    loader.loadRichFilterBuilder.mockReturnValue(gate.promise);
    renderHost();
    expect(screen.getByTestId("control-slot")).toBeEmptyDOMElement();
    expect(screen.getByTestId("row-slot")).toBeEmptyDOMElement();
    gate.resolve(fakeModule(useRichFilterBuilder));
    expect(await screen.findByTestId("fake-filter-control")).toBeInTheDocument();
    expect(screen.getByTestId("fake-filter-row")).toBeInTheDocument();
    expect(useRichFilterBuilder).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceSlug: "acme",
        projectId: "",
        entity: { scope: "workspace" },
        hideCycle: true,
        hideModule: true,
        temporary: true,
      })
    );
  });

  it("reports the live expression for the body read", async () => {
    const seen: RichExpression[] = [];
    loader.loadRichFilterBuilder.mockResolvedValue(fakeModule(fakeBuilder(), () => ({ filters: '{"and":[]}' })));
    renderHost((expression) => seen.push(expression));
    await waitFor(() => expect(seen).toEqual([{ filters: '{"and":[]}' }]));
  });

  it("drops a late chunk resolution after unmount", async () => {
    const gate = deferred<RichFilterBuilderModule>();
    const useRichFilterBuilder = fakeBuilder();
    loader.loadRichFilterBuilder.mockReturnValue(gate.promise);
    const view = render(
      <WorkspaceDefaultFilters workspaceSlug="acme" onExpressionChange={() => {}}>
        {(slots) => (
          <>
            <div data-testid="control-slot">{slots.control}</div>
            <div data-testid="row-slot">{slots.row}</div>
          </>
        )}
      </WorkspaceDefaultFilters>
    );
    view.unmount();
    gate.resolve(fakeModule(useRichFilterBuilder));
    await gate.promise;
    expect(useRichFilterBuilder).not.toHaveBeenCalled();
  });

  it("reports a chunk failure in the row slot", async () => {
    loader.loadRichFilterBuilder.mockRejectedValue(new Error("chunk down"));
    renderHost();
    expect(await screen.findByRole("alert")).toHaveTextContent("Filters could not be loaded.");
    expect(screen.getByTestId("control-slot")).toBeEmptyDOMElement();
  });
});
