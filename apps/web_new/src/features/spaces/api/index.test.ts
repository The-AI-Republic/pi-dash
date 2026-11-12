// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import * as spacesApi from "./index.js";

describe("spaces API barrel", () => {
  it("re-exports the public runtime, keys and query options", () => {
    expect(spacesApi.createSpaceRuntime).toBeTypeOf("function");
    expect(spacesApi.spaceRuntime.client).toBeDefined();
    expect(spacesApi.spaceKeys.instance).toEqual(["spaces", "instance"]);
    expect(spacesApi.readSpaceViewer).toBeTypeOf("function");
    expect(spacesApi.resolveLegacySpaceAnchor).toBeTypeOf("function");
    expect(spacesApi.spaceInstanceQueryOptions).toBeTypeOf("function");
    expect(spacesApi.spaceIssuesQueryOptions).toBeTypeOf("function");
    expect(spacesApi.spaceSettingsQueryOptions).toBeTypeOf("function");
    expect(spacesApi.spaceStatesQueryOptions).toBeTypeOf("function");
    expect(spacesApi.spaceViewerQueryOptions).toBeTypeOf("function");
  });
});
