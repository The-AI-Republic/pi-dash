// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared setup for web_new component tests: jest-dom matchers and DOM
// cleanup between tests (mirrors @pidash/kit).
import "@testing-library/jest-dom/vitest";
import * as matchers from "@testing-library/jest-dom/matchers";
import { cleanup } from "@testing-library/react";
import { afterEach, expect } from "vitest";

expect.extend(matchers);

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

afterEach(() => {
  // Node-env suites never mount; only jsdom suites have a DOM to clean.
  if (typeof globalThis.document !== "undefined") {
    cleanup();
  }
});
