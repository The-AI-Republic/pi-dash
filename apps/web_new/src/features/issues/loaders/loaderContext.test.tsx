// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";
import { IssuesLoaderProvider, useIssuesLoader } from "./loaderContext.js";

function Probe({ onLoader }: { onLoader: (loader: ReturnType<typeof useIssuesLoader>) => void }): React.ReactElement {
  const loader = useIssuesLoader();
  onLoader(loader);
  return <p>{`loading:${loader.isMutationLoading}`}</p>;
}

function renderProbe(): {
  text: () => string;
  run: <T>(run: () => Promise<T>) => Promise<T>;
} {
  let run: (<T>(run: () => Promise<T>) => Promise<T>) | null = null;
  render(
    <IssuesLoaderProvider>
      <Probe
        onLoader={(loader) => {
          run = loader.runWithMutationLoader;
        }}
      />
    </IssuesLoaderProvider>
  );
  return {
    text: () => screen.getByText(/^loading:/).textContent ?? "",
    run: <T,>(task: () => Promise<T>): Promise<T> => {
      if (!run) throw new Error("loader not captured");
      return (run as <T>(run: () => Promise<T>) => Promise<T>)(task);
    },
  };
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void; reject: (error: unknown) => void } {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("IssuesLoaderProvider", () => {
  it("starts idle and renders its children", () => {
    const h = renderProbe();
    expect(h.text()).toBe("loading:false");
  });

  it("raises the loader while a wrapped write runs, then drops it", async () => {
    const h = renderProbe();
    const gate = deferred<string>();
    const settled = h.run(() => gate.promise);
    await waitFor(() => expect(h.text()).toBe("loading:true"));
    gate.resolve("done");
    await expect(settled).resolves.toBe("done");
    await waitFor(() => expect(h.text()).toBe("loading:false"));
  });

  it("drops the loader and rethrows when the write fails", async () => {
    const h = renderProbe();
    const gate = deferred<string>();
    const settled = h.run(() => gate.promise);
    await waitFor(() => expect(h.text()).toBe("loading:true"));
    gate.reject(new Error("offline"));
    await expect(settled).rejects.toThrow("offline");
    await waitFor(() => expect(h.text()).toBe("loading:false"));
  });

  it("stays raised until every concurrent write settles", async () => {
    const h = renderProbe();
    const first = deferred<string>();
    const second = deferred<string>();
    const settledFirst = h.run(() => first.promise);
    const settledSecond = h.run(() => second.promise);
    await waitFor(() => expect(h.text()).toBe("loading:true"));
    first.resolve("one");
    await expect(settledFirst).resolves.toBe("one");
    expect(h.text()).toBe("loading:true");
    second.resolve("two");
    await expect(settledSecond).resolves.toBe("two");
    await waitFor(() => expect(h.text()).toBe("loading:false"));
  });

  it("reuses an outer scope instead of splitting the count with a nested one", async () => {
    const outerGates: Array<{ run: <T>(task: () => Promise<T>) => Promise<T> }> = [];
    const innerGates: Array<{ run: <T>(task: () => Promise<T>) => Promise<T> }> = [];
    render(
      <IssuesLoaderProvider>
        <Probe
          onLoader={(loader) => {
            outerGates.push({ run: loader.runWithMutationLoader });
          }}
        />
        <IssuesLoaderProvider>
          <Probe
            onLoader={(loader) => {
              innerGates.push({ run: loader.runWithMutationLoader });
            }}
          />
        </IssuesLoaderProvider>
      </IssuesLoaderProvider>
    );
    const outer = outerGates[outerGates.length - 1]!;
    const inner = innerGates[innerGates.length - 1]!;
    expect(inner.run).toBe(outer.run);
    const gate = deferred<string>();
    const settled = outer.run(() => gate.promise);
    await waitFor(() => expect(screen.getAllByText("loading:true")).toHaveLength(2));
    gate.resolve("done");
    await expect(settled).resolves.toBe("done");
    await waitFor(() => expect(screen.getAllByText("loading:false")).toHaveLength(2));
  });

  it("throws outside the provider", () => {
    expect(() =>
      render(
        <Probe
          onLoader={() => {
            throw new Error("must not run");
          }}
        />
      )
    ).toThrow("useIssuesLoader must be used under IssuesLoaderProvider.");
  });
});
