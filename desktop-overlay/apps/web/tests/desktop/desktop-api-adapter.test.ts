import axios from "axios";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getDesktopApiAdapter } from "../../../../packages/services/src/desktop-api-adapter";

const api = "https://api.example.test";
const invoke = vi.fn();

function frame(head: unknown, body: Uint8Array): ArrayBuffer {
  const encoded = new TextEncoder().encode(JSON.stringify(head));
  const out = new Uint8Array(4 + encoded.length + body.length);
  new DataView(out.buffer).setUint32(0, encoded.length);
  out.set(encoded, 4);
  out.set(body, 4 + encoded.length);
  return out.buffer;
}
function response(status = 200, data: unknown = { id: "user-1" }, body?: Uint8Array) {
  return frame(
    { status, statusText: String(status), headers: [["content-type", "application/json"]] },
    body ?? new TextEncoder().encode(JSON.stringify(data))
  );
}
function sent(call = 0) {
  const [command, bytes] = invoke.mock.calls.filter(([name]) => name === "desktop_api_request")[call];
  const length = new DataView(bytes.buffer, bytes.byteOffset).getUint32(0);
  return {
    command,
    request: JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + length))),
    body: bytes.subarray(4 + length) as Uint8Array,
  };
}
function client() {
  return axios.create({ baseURL: api, adapter: getDesktopApiAdapter() });
}

beforeEach(() => {
  Object.assign(window, { __PIDASH_NATIVE_HTTP__: api, __TAURI__: { core: { invoke } } });
  invoke.mockReset().mockResolvedValue(response());
});
afterEach(() => {
  delete (window as any).__PIDASH_NATIVE_HTTP__;
  delete (window as any).__TAURI__;
});

describe("desktop API cookie transport", () => {
  it("leaves browsers and older desktop binaries on their normal adapter", () => {
    delete (window as any).__PIDASH_NATIVE_HTTP__;
    expect(getDesktopApiAdapter()).toBeUndefined();
    (window as any).__PIDASH_NATIVE_HTTP__ = api;
    delete (window as any).__TAURI__;
    expect(getDesktopApiAdapter()).toBeUndefined();
  });

  it("keeps presigned storage uploads and relative URLs on the browser adapter", async () => {
    const browser = vi.fn(async (config) => ({ data: "uploaded", status: 200, statusText: "OK", headers: {}, config }));
    const previous = axios.defaults.adapter;
    axios.defaults.adapter = browser;
    try {
      await client().post("https://storage.example.test/upload", "file", { withCredentials: false });
      // Relative URLs resolve against the page, as XHR would, not the API.
      await axios.create({ adapter: getDesktopApiAdapter() }).get("/api/users/me/");
      expect(browser).toHaveBeenCalledTimes(2);
      expect(invoke).not.toHaveBeenCalled();
    } finally {
      axios.defaults.adapter = previous;
    }
  });

  it("sends query parameters and JSON as a raw IPC frame and decodes the response", async () => {
    const result = await client().post("/api/issues/", { title: "你好" }, { params: { page: 2 } });
    expect(result.data).toEqual({ id: "user-1" });
    const { command, request, body } = sent();
    expect(command).toBe("desktop_api_request");
    expect(request.url).toBe(`${api}/api/issues/?page=2`);
    expect(request.method).toBe("POST");
    expect(request.hasBody).toBe(true);
    expect(request.timeoutMs).toBeNull();
    expect(new TextDecoder().decode(body)).toBe('{"title":"你好"}');
    expect(request.headers).toContainEqual(["Content-Type", "application/json"]);
  });

  it("marks body-less requests", async () => {
    await client().get("/api/users/me/");
    expect(sent().request.hasBody).toBe(false);
    expect(sent().body).toHaveLength(0);
  });

  it("rejects HTTP auth errors so existing refresh interceptors can replay requests", async () => {
    invoke.mockResolvedValueOnce(response(401, { detail: "expired" }));
    await expect(client().get("/api/users/me/")).rejects.toMatchObject({
      isAxiosError: true,
      response: { status: 401, data: { detail: "expired" } },
    });
  });

  it("preserves validateStatus:null used by currentUser's signed-out probe", async () => {
    invoke.mockResolvedValueOnce(response(401, { detail: "signed_out" }));
    await expect(client().get("/api/users/me/", { validateStatus: null })).resolves.toMatchObject({
      status: 401,
      data: { detail: "signed_out" },
    });
  });

  it("uploads multipart data with the generated boundary", async () => {
    const form = new FormData();
    form.append("name", "attachment");
    form.append("file", new File(["test contents"], "test.txt", { type: "text/plain" }));
    await client().post("/api/assets/", form, { headers: { "Content-Type": "multipart/form-data" } });
    const { request, body: bytes } = sent();
    const type = request.headers.find(([name]: [string, string]) => name.toLowerCase() === "content-type")[1];
    expect(type).toMatch(/^multipart\/form-data; boundary=/);
    const body = new TextDecoder().decode(bytes);
    expect(body).toContain('filename="test.txt"');
    expect(body).toContain("test contents");
    expect(body).toContain(type.split("boundary=")[1]);
  });

  it("preserves binary downloads", async () => {
    invoke.mockResolvedValueOnce(response(200, null, new Uint8Array([0, 128, 255])));
    const result = await client().get("/api/export/", { responseType: "arraybuffer" });
    expect(Array.from(new Uint8Array(result.data))).toEqual([0, 128, 255]);
  });

  it("honors cancellation and stops the native request", async () => {
    invoke.mockImplementation((command) =>
      command === "desktop_api_request" ? new Promise(() => {}) : Promise.resolve()
    );
    const controller = new AbortController();
    const pending = client().get("/api/users/me/", { signal: controller.signal });
    await vi.waitFor(() => expect(invoke).toHaveBeenCalled());
    controller.abort();
    await expect(pending).rejects.toMatchObject({ code: "ERR_CANCELED" });
    expect(invoke).toHaveBeenCalledWith("desktop_api_cancel", { id: sent().request.id });
  });

  it("passes explicit timeouts natively and reports them as timeouts", async () => {
    invoke.mockRejectedValueOnce("API request timed out");
    await expect(client().get("/api/users/me/", { timeout: 5000 })).rejects.toMatchObject({
      code: "ECONNABORTED",
    });
    expect(sent().request.timeoutMs).toBe(5000);
  });

  it("treats transport failures as network errors, without inventing a 401", async () => {
    invoke.mockRejectedValue("API request failed");
    const error = await client()
      .get("/api/users/me/")
      .catch((caught) => caught);
    expect(error.code).toBe("ERR_NETWORK");
    expect(error.response).toBeUndefined();
  });
});
