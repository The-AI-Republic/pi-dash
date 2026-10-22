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

import type { FieldConfigRegistry, IssuesEntity, RichFilterBuilderModule } from "../../issues/index.js";
// Test-only deep imports (checks skip test files): the registry class
// and snapshot helpers the mocked module carries for the host.
import { buildViewCreate, savedViewKeys } from "../../issues/filters/rich/views.js";
import { FieldConfigRegistry as RealRegistry } from "../../issues/filters/rich/temporary.js";
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

function fakeModule(useRichFilterBuilder: ReturnType<typeof vi.fn>): RichFilterBuilderModule {
  return {
    useRichFilterBuilder,
    serializeRichExpression: () => ({}),
    FieldConfigRegistry: RealRegistry,
    buildViewCreate,
    savedViewKeys,
  };
}

function fakeBuilder(): ReturnType<typeof vi.fn> {
  return vi.fn(
    (): RichFilterBuilderParts => ({
      control: <span data-testid="fake-filter-control" />,
      row: <span data-testid="fake-filter-row" />,
      instanceId: "TEMP-9",
    })
  );
}

function renderLoader(registryRef: { current: FieldConfigRegistry | null }): void {
  render(
    <FilterSectionLoader
      workspaceSlug="acme"
      projectId={PROJECT}
      entity={ENTITY}
      hideCycle={false}
      hideModule={false}
      registryRef={registryRef}
    />
  );
}

afterEach(() => {
  loader.loadRichFilterBuilder.mockReset();
});

describe("FilterSectionLoader", () => {
  it("shows a loading note while the chunk loads, then the builder with its registry", async () => {
    const gate = deferred<RichFilterBuilderModule>();
    const useRichFilterBuilder = fakeBuilder();
    loader.loadRichFilterBuilder.mockReturnValue(gate.promise);
    const registryRef: { current: FieldConfigRegistry | null } = { current: null };
    renderLoader(registryRef);
    expect(screen.getByText("Loading filters…")).toBeInTheDocument();
    expect(loader.loadRichFilterBuilder).toHaveBeenCalledTimes(1);
    gate.resolve(fakeModule(useRichFilterBuilder));
    expect(await screen.findByTestId("fake-filter-control")).toBeInTheDocument();
    expect(screen.getByTestId("fake-filter-row")).toBeInTheDocument();
    // The ref lands in the host's passive effect, which can trail the
    // commit findBy observes under parallel load; poll for it.
    await waitFor(() => expect(registryRef.current).toBeInstanceOf(RealRegistry));
    expect(useRichFilterBuilder).toHaveBeenCalledWith({
      workspaceSlug: "acme",
      projectId: PROJECT,
      entity: ENTITY,
      hideCycle: false,
      hideModule: false,
      temporary: true,
      temporaryRegistry: registryRef.current,
    });
  });

  it("shows an error note when the chunk fails and leaves the ref empty", async () => {
    loader.loadRichFilterBuilder.mockRejectedValue(new Error("chunk gone"));
    const registryRef: { current: FieldConfigRegistry | null } = { current: null };
    renderLoader(registryRef);
    expect(await screen.findByRole("alert")).toHaveTextContent("Filters could not be loaded.");
    expect(registryRef.current).toBeNull();
  });

  it("releases the registry on unmount", async () => {
    const gate = deferred<RichFilterBuilderModule>();
    loader.loadRichFilterBuilder.mockReturnValue(gate.promise);
    const registryRef: { current: FieldConfigRegistry | null } = { current: null };
    const view = render(
      <FilterSectionLoader
        workspaceSlug="acme"
        projectId={PROJECT}
        entity={ENTITY}
        hideCycle={false}
        hideModule={false}
        registryRef={registryRef}
      />
    );
    gate.resolve(fakeModule(fakeBuilder()));
    expect(await screen.findByTestId("fake-filter-control")).toBeInTheDocument();
    // Same commit/effect race as above; poll for the ref before unmounting.
    await waitFor(() => expect(registryRef.current).toBeInstanceOf(RealRegistry));
    view.unmount();
    expect(registryRef.current).toBeNull();
  });

  it("ignores a settled load after unmount", async () => {
    const resolveGate = deferred<RichFilterBuilderModule>();
    loader.loadRichFilterBuilder.mockReturnValueOnce(resolveGate.promise);
    const resolveRef: { current: FieldConfigRegistry | null } = { current: null };
    const first = render(
      <FilterSectionLoader
        workspaceSlug="acme"
        projectId={PROJECT}
        entity={ENTITY}
        hideCycle={false}
        hideModule={false}
        registryRef={resolveRef}
      />
    );
    first.unmount();
    const useRichFilterBuilder = fakeBuilder();
    resolveGate.resolve(fakeModule(useRichFilterBuilder));
    await resolveGate.promise;
    await new Promise((done) => setTimeout(done, 0));
    expect(useRichFilterBuilder).not.toHaveBeenCalled();
    expect(resolveRef.current).toBeNull();
    expect(screen.queryByTestId("fake-filter-control")).not.toBeInTheDocument();

    const rejectGate = deferred<RichFilterBuilderModule>();
    loader.loadRichFilterBuilder.mockReturnValueOnce(rejectGate.promise);
    const rejectRef: { current: FieldConfigRegistry | null } = { current: null };
    const second = render(
      <FilterSectionLoader
        workspaceSlug="acme"
        projectId={PROJECT}
        entity={ENTITY}
        hideCycle={false}
        hideModule={false}
        registryRef={rejectRef}
      />
    );
    second.unmount();
    rejectGate.reject(new Error("chunk gone"));
    await expect(rejectGate.promise).rejects.toThrow("chunk gone");
    await new Promise((done) => setTimeout(done, 0));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
