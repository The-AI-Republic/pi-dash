// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract-test transport redirect handling against a loopback
// stub (no Django): bodies forward, then drop when a redirect
// turns the follow-up into a GET.
import http from "node:http";
import type { AddressInfo } from "node:net";
import { describe, expect, it } from "vitest";
import { contractTransport } from "./setup.js";

describe("contractTransport", () => {
  it("forwards bodies and drops them when a redirect becomes GET", async () => {
    const seen: { method: string | undefined; hasBody: boolean }[] = [];
    const server = http.createServer((request, response) => {
      let size = 0;
      request.on("data", (chunk: Buffer) => {
        size += chunk.length;
      });
      request.on("end", () => {
        if (request.url === "/bounce") {
          response.writeHead(303, { location: "/landed" });
          response.end();
          return;
        }
        seen.push({ method: request.method, hasBody: size > 0 });
        response.writeHead(200, { "content-type": "text/plain" });
        response.end("ok");
      });
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    try {
      const address = server.address() as AddressInfo;
      const base = `http://127.0.0.1:${address.port}`;
      const transport = contractTransport();
      const form = new FormData();
      form.append("file", new Blob(["audio"]), "a.webm");
      await transport(`${base}/bounce`, { method: "POST", headers: {}, formData: form });
      await transport(`${base}/bounce`, { method: "POST", headers: {}, body: '{"a":1}' });
      expect(seen).toEqual([
        { method: "GET", hasBody: false },
        { method: "GET", hasBody: false },
      ]);
    } finally {
      await new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
    }
  });
});
