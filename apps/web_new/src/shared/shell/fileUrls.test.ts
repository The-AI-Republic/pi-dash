// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { resolveFileUrl } from "./fileUrls.js";

describe("resolveFileUrl", () => {
  it("passes absolute, data and blob legs through untouched", () => {
    expect(resolveFileUrl("https://cdn.example/logo.png", "https://api.example")).toBe("https://cdn.example/logo.png");
    expect(resolveFileUrl("http://cdn.example/logo.png", "https://api.example")).toBe("http://cdn.example/logo.png");
    expect(resolveFileUrl("data:image/png;base64,AAA", "https://api.example")).toBe("data:image/png;base64,AAA");
    expect(resolveFileUrl("blob:https://app.example/1", "https://api.example")).toBe("blob:https://app.example/1");
  });

  it("joins relative legs to the API base without doubling slashes", () => {
    expect(resolveFileUrl("/media/logo.png", "https://api.example/")).toBe("https://api.example/media/logo.png");
    expect(resolveFileUrl("media/logo.png", "https://api.example")).toBe("https://api.example/media/logo.png");
  });

  it("resolves empty legs to null", () => {
    expect(resolveFileUrl(null, "https://api.example")).toBeNull();
    expect(resolveFileUrl(undefined, "https://api.example")).toBeNull();
    expect(resolveFileUrl("", "https://api.example")).toBeNull();
  });
});
