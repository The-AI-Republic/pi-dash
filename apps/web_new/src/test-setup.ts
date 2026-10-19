// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared setup for web_new component tests: jest-dom matchers and DOM
// cleanup between tests (mirrors @pidash/kit).
import "@testing-library/jest-dom/vitest";
import * as matchers from "@testing-library/jest-dom/matchers";
import { cleanup } from "@testing-library/react";
import { afterEach, expect } from "vitest";

import { installTestStorage } from "./testSupport/memoryStorage.js";

expect.extend(matchers);

// Guarantee: every jsdom test file starts with a fresh in-memory profile,
// whatever Node's own global localStorage is (Node 26 leaves an undefined
// one without --localstorage-file and a process-wide one with it). Suites
// stub it per test; Node-env suites get no storage.
installTestStorage(globalThis as { document?: unknown; localStorage?: unknown });

// jsdom lacks Range geometry: ProseMirror scrolls the selection into view
// on every transaction, which reads rects off a Range. The stubs report
// position zero, which is all the editor needs here.
if (typeof globalThis.Range !== "undefined") {
  const rangePrototype = globalThis.Range.prototype as Range & {
    getClientRects?: () => DOMRectList;
    getBoundingClientRect?: () => DOMRect;
  };
  if (rangePrototype.getClientRects === undefined) {
    rangePrototype.getClientRects = function getClientRects(): DOMRectList {
      return [] as unknown as DOMRectList;
    };
  }
  if (rangePrototype.getBoundingClientRect === undefined) {
    rangePrototype.getBoundingClientRect = function getBoundingClientRect(): DOMRect {
      return new globalThis.DOMRect(0, 0, 0, 0);
    };
  }
}

// jsdom has no hit testing. ProseMirror asks for the element under the
// pointer while handling typed transactions; without a stub every editor
// keystroke throws and the document never changes. Suites that need a
// real target install `elementFromPointInEditor`.
declare global {
  interface Document {
    /** Optional hit-test target for editor typing under jsdom. */
    elementFromPointInEditor?: ((x: number, y: number) => Element | null) | undefined;
  }
}

if (typeof document !== "undefined" && typeof document.elementFromPoint !== "function") {
  document.elementFromPoint = (x: number, y: number) => document.elementFromPointInEditor?.(x, y) ?? document.body;
}

afterEach(() => {
  // Node-env suites never mount; only jsdom suites have a DOM to clean.
  if (typeof globalThis.document !== "undefined") {
    cleanup();
  }
});
