/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it, vi } from "vitest";
import { AssistantService } from "@pi-dash/services";

// Wire contract with apps/api/pi_dash/assistant/views/transcribe.py, which
// reads the recording from `request.FILES["file"]` (the OpenAI-compatible field
// name). The hook tests mock this service wholesale, so without this test a
// field-name drift ships green and every real dictation fails with `no_audio`.
describe("AssistantService.transcribeAudio", () => {
  it("uploads the recording as the multipart field `file`", async () => {
    const service = new AssistantService("http://api.test");
    const post = vi
      .spyOn(
        (service as unknown as { axiosInstance: { post: (...a: unknown[]) => Promise<unknown> } }).axiosInstance,
        "post"
      )
      .mockResolvedValue({ data: { text: "hello" } });

    const result = await service.transcribeAudio(new Blob(["x"], { type: "audio/webm" }));

    expect(result).toEqual({ text: "hello" });
    const [url, body] = post.mock.calls[0] as [string, FormData];
    expect(url).toBe("/api/users/me/ai-assistant/transcribe/");
    expect(body).toBeInstanceOf(FormData);
    expect([...body.keys()]).toEqual(["file"]);
    expect(body.get("file")).toBeInstanceOf(Blob);
  });

  it("rejects with the API error body so callers can branch on `error`", async () => {
    const service = new AssistantService("http://api.test");
    vi.spyOn(
      (service as unknown as { axiosInstance: { post: (...a: unknown[]) => Promise<unknown> } }).axiosInstance,
      "post"
    ).mockRejectedValue({
      response: { data: { error: "stt_config_missing", detail: "Configure dictation in Settings." } },
    });

    await expect(service.transcribeAudio(new Blob(["x"]))).rejects.toEqual({
      error: "stt_config_missing",
      detail: "Configure dictation in Settings.",
    });
  });
});
