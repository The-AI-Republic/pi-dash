// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { LANGUAGE_STORAGE_KEY } from "../../../core/i18n/locales.js";

import { SpaceLocaleGate } from "./SpaceLocaleGate.js";

afterEach(() => {
  globalThis.localStorage?.clear();
  document.documentElement.removeAttribute("lang");
});

describe("SpaceLocaleGate", () => {
  it("renders nothing and heals the stored locale on mount", () => {
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    const { container } = render(<SpaceLocaleGate />);
    expect(container).toBeEmptyDOMElement();
    expect(globalThis.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
    expect(document.documentElement.getAttribute("lang")).toBe("en");
  });
});
