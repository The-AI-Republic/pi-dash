/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import { AssistantService } from "@pi-dash/services";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("AssistantService.transcribeAudio", () => {
  it("uploads the recording under the multipart field `file` that the transcribe view reads", async () => {
    const service = new AssistantService("http://api.test");
    const post = vi
      .spyOn(service as unknown as { post: (url: string, data: unknown) => Promise<unknown> }, "post")
      .mockResolvedValue({ data: { text: "hi" } });

    const blob = new Blob(["audio"], { type: "audio/webm" });
    await expect(service.transcribeAudio(blob)).resolves.toEqual({ text: "hi" });

    expect(post).toHaveBeenCalledTimes(1);
    const [url, form] = post.mock.calls[0] as [string, FormData];
    expect(url).toBe("/api/users/me/ai-assistant/transcribe/");
    expect(form).toBeInstanceOf(FormData);
    const file = form.get("file");
    expect(file).toBeInstanceOf(Blob);
    expect(form.has("audio")).toBe(false);
  });

  it("rejects with the API error body so callers can branch on `error`", async () => {
    const service = new AssistantService("http://api.test");
    vi.spyOn(service as unknown as { post: () => Promise<unknown> }, "post").mockRejectedValue({
      response: { data: { error: "stt_config_missing", detail: "Configure dictation in Settings." } },
    });

    await expect(service.transcribeAudio(new Blob(["a"]))).rejects.toEqual({
      error: "stt_config_missing",
      detail: "Configure dictation in Settings.",
    });
  });
});
