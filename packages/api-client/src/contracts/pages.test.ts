// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page live-operations contracts against a stub client: the liveness
// probe, document conversion and printable export plus their failure
// readers. Live reachability stays in the contract suite.
import { describe, expect, it } from "vitest";
import type { ApiClient, TransportResponse } from "../client.js";
import { ApiError } from "../errors.js";
import {
  convertDocument,
  convertDocumentErrorFrom,
  exportPagePdf,
  getLiveHealth,
  pdfExportErrorFrom,
} from "./pages.js";

function stubClient(impl: {
  getJson?: ApiClient["getJson"];
  sendJson?: ApiClient["sendJson"];
  post?: ApiClient["post"];
}): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

function stubResponse(headers: Record<string, string>, body: string | Uint8Array): TransportResponse {
  const bytes = typeof body === "string" ? new TextEncoder().encode(body) : body;
  const lower: Record<string, string> = {};
  for (const [key, value] of Object.entries(headers)) lower[key.toLowerCase()] = value;
  const copy = new Uint8Array(bytes.byteLength);
  copy.set(bytes);
  return {
    status: 200,
    url: "http://live.test/live/pdf-export/",
    headers: { get: (name: string) => lower[name.toLowerCase()] ?? null },
    text: async () => new TextDecoder().decode(copy),
    arrayBuffer: async () => copy.buffer,
  };
}

describe("live health", () => {
  it("reads the probe answer", async () => {
    const calls: string[] = [];
    const client = stubClient({
      getJson: (async (url: string) => {
        calls.push(url);
        return { status: "OK", timestamp: "2026-10-22T11:40:29.559Z", version: "1.0.0" };
      }) as ApiClient["getJson"],
    });
    const health = await getLiveHealth(client);
    expect(calls).toEqual(["/live/health/"]);
    expect(health).toEqual({ status: "OK", timestamp: "2026-10-22T11:40:29.559Z", version: "1.0.0" });
  });
});

describe("convert document", () => {
  it("posts markup with the variant and optionals", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return {
          data: {
            description_json: { type: "doc", content: [] },
            description_binary: "AQe/",
            description_html: "<p>x</p>",
          },
          status: 200,
          url,
        };
      }) as ApiClient["sendJson"],
    });
    const converted = await convertDocument(client, {
      description_html: "<p>x</p>",
      variant: "rich",
      description_binary: "AQe/",
      title: "Probe",
    });
    expect(calls[0]?.url).toBe("/live/convert-document/");
    expect(calls[0]?.json).toEqual({
      description_html: "<p>x</p>",
      variant: "rich",
      description_binary: "AQe/",
      title: "Probe",
    });
    expect(converted.description_binary).toBe("AQe/");
  });

  it("posts without the optionals", async () => {
    const calls: { json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, _url: string, options: { json?: unknown }) => {
        calls.push({ json: options.json });
        return {
          data: { description_json: { type: "doc" }, description_binary: "AQe/", description_html: "<p>x</p>" },
          status: 200,
          url: "/live/convert-document/",
        };
      }) as ApiClient["sendJson"],
    });
    await convertDocument(client, { description_html: "<p>x</p>", variant: "document" });
    expect(calls[0]?.json).toEqual({
      description_html: "<p>x</p>",
      variant: "document",
      description_binary: undefined,
      title: undefined,
    });
  });
});

describe("convert document failures", () => {
  it("reads the violations off a 400", () => {
    const failure = convertDocumentErrorFrom(
      new ApiError({
        status: 400,
        code: "http",
        message: "Validation error",
        body: {
          message: "Validation error",
          context: { validationErrors: [{ path: "description_html", message: "Content must be valid HTML" }] },
        },
      })
    );
    expect(failure).toEqual({
      message: "Validation error",
      violations: [{ path: "description_html", message: "Content must be valid HTML" }],
      status: 400,
    });
  });

  it("reads an empty violation list without context", () => {
    const failure = convertDocumentErrorFrom(
      new ApiError({ status: 500, code: "http", message: "Internal server error.", body: { message: "x" } })
    );
    expect(failure).toEqual({ message: "x", violations: [], status: 500 });
  });

  it("reads an empty violation list when the context names none", () => {
    const failure = convertDocumentErrorFrom(
      new ApiError({
        status: 400,
        code: "http",
        message: "Validation error",
        body: { message: "Validation error", context: {} },
      })
    );
    expect(failure?.violations).toEqual([]);
  });

  it("ignores foreign failures", () => {
    expect(convertDocumentErrorFrom(new Error("boom"))).toBeNull();
    expect(convertDocumentErrorFrom(null)).toBeNull();
    expect(
      convertDocumentErrorFrom(new ApiError({ status: 400, code: "http", message: "x", body: { error: "x" } }))
    ).toBeNull();
    expect(convertDocumentErrorFrom(new ApiError({ status: 400, code: "http", message: "x" }))).toBeNull();
  });
});

describe("page pdf export", () => {
  const input = {
    pageId: "f4dff0fa-14b0-49df-8eda-121faf8d21ce",
    workspaceSlug: "acme",
    projectId: "5af868d5-f7c6-4494-9da0-b2185b57a3db",
    title: "Probe",
    fileName: "probe.pdf",
    noAssets: true,
  };

  it("posts the export body and returns the named bytes", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      post: (async (url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return stubResponse(
          {
            "content-type": "application/pdf",
            "content-disposition": `attachment; filename="page.pdf"; filename*=UTF-8''probe.pdf`,
          },
          new Uint8Array([0x25, 0x50, 0x44, 0x46])
        );
      }) as ApiClient["post"],
    });
    const exported = await exportPagePdf(client, input);
    expect(calls[0]?.url).toBe("/live/pdf-export/");
    expect(calls[0]?.json).toEqual({
      pageId: input.pageId,
      workspaceSlug: "acme",
      projectId: input.projectId,
      title: "Probe",
      author: undefined,
      subject: undefined,
      pageSize: undefined,
      pageOrientation: undefined,
      fileName: "probe.pdf",
      noAssets: true,
    });
    expect(exported.filename).toBe("probe.pdf");
    expect(exported.contentType).toBe("application/pdf");
    expect([...exported.bytes]).toEqual([0x25, 0x50, 0x44, 0x46]);
  });

  it("falls back to the quoted filename", async () => {
    const client = stubClient({
      post: (async () =>
        stubResponse(
          { "content-type": "application/pdf", "content-disposition": `attachment; filename="page.pdf"` },
          new Uint8Array([1])
        )) as ApiClient["post"],
    });
    const exported = await exportPagePdf(client, { pageId: "p", workspaceSlug: "acme" });
    expect(exported.filename).toBe("page.pdf");
  });

  it("decodes a percent-encoded extended filename", async () => {
    const client = stubClient({
      post: (async () =>
        stubResponse(
          {
            "content-type": "application/pdf",
            "content-disposition": `attachment; filename*=UTF-8''caf%C3%A9.pdf`,
          },
          new Uint8Array([1])
        )) as ApiClient["post"],
    });
    const exported = await exportPagePdf(client, { pageId: "p", workspaceSlug: "acme" });
    expect(exported.filename).toBe("café.pdf");
  });

  it("reads an empty filename when the disposition is missing or bare", async () => {
    for (const headers of [
      { "content-type": "application/pdf" },
      { "content-type": "application/pdf", "content-disposition": "attachment" },
    ]) {
      const client = stubClient({
        post: (async () => stubResponse(headers, new Uint8Array([1]))) as ApiClient["post"],
      });
      const exported = await exportPagePdf(client, { pageId: "p", workspaceSlug: "acme" });
      expect(exported.filename).toBe("");
    }
  });

  it("reads an empty filename when the extended name is malformed", async () => {
    const client = stubClient({
      post: (async () =>
        stubResponse(
          { "content-type": "application/pdf", "content-disposition": `attachment; filename*=UTF-8''%` },
          new Uint8Array([1])
        )) as ApiClient["post"],
    });
    const exported = await exportPagePdf(client, { pageId: "p", workspaceSlug: "acme" });
    expect(exported.filename).toBe("");
  });

  it("rejects a non-PDF answer as a contract violation", async () => {
    for (const headers of [{ "content-type": "application/json" }, {}]) {
      const client = stubClient({
        post: (async () => stubResponse(headers, "{}")) as ApiClient["post"],
      });
      await expect(exportPagePdf(client, { pageId: "p", workspaceSlug: "acme" })).rejects.toMatchObject({
        code: "contract",
      });
    }
  });

  it("rejects an unreadable body as a contract violation", async () => {
    const response = stubResponse({ "content-type": "application/pdf" }, new Uint8Array([1]));
    const withoutBytes: TransportResponse = {
      status: response.status,
      url: response.url,
      headers: response.headers,
      text: response.text,
    };
    const client = stubClient({
      post: (async () => withoutBytes) as ApiClient["post"],
    });
    const error: unknown = await exportPagePdf(client, { pageId: "p", workspaceSlug: "acme" }).then(
      () => null,
      (rejected: unknown) => rejected
    );
    expect(error).toMatchObject({ code: "contract" });
    expect(error instanceof Error ? error.message : "").toContain("no readable bytes");
  });
});

describe("page pdf export failures", () => {
  function refusal(status: number): ApiError {
    return new ApiError({ status, code: "http", message: "refused", body: { message: "refused" } });
  }

  it.each([
    [401, "auth"],
    [400, "validation"],
    [404, "fetch"],
    [502, "fetch"],
    [504, "timeout"],
    [500, "generation"],
    [503, "generation"],
  ])("maps status %i to %s", (status, kind) => {
    expect(pdfExportErrorFrom(refusal(status))).toEqual({ kind, message: "refused", status });
  });

  it("ignores foreign failures", () => {
    expect(pdfExportErrorFrom(new Error("boom"))).toBeNull();
    expect(pdfExportErrorFrom(null)).toBeNull();
    expect(pdfExportErrorFrom(refusal(401).message)).toBeNull();
    expect(
      pdfExportErrorFrom(new ApiError({ status: 401, code: "http", message: "x", body: { error: "x" } }))
    ).toBeNull();
  });
});
