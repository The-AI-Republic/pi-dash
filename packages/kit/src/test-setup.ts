// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared setup for @pidash/kit component tests: jest-dom matchers and the
// design tokens so computed-style assertions see the real values.
//
// Matchers are registered explicitly against this package's vitest
// instance. The `/vitest` entry point extends whatever `vitest` resolves
// from jest-dom's location, which is not guaranteed to be the same module
// instance the workers use; importing from `@testing-library/jest-dom`
// here and extending the local `expect` always lines up.
import "@testing-library/jest-dom/vitest";
import * as matchers from "@testing-library/jest-dom/matchers";
import { cleanup } from "@testing-library/react";
import { afterEach, expect } from "vitest";
import "./tokens.css";

expect.extend(matchers);

afterEach(() => {
  cleanup();
});
