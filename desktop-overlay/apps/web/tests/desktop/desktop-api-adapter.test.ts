import axios from "axios";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getDesktopApiAdapter } from "../../../../packages/services/src/desktop-api-adapter";

const api = "https://api.example.test";
const invoke = vi.fn();
function response(status = 200, data: unknown = { id: "user-1" }) {
  return {
    status,
    statusText: String(status),
    headers: [["content-type", "application/json"]],
    body: Array.from(new TextEncoder().encode(JSON.stringify(data))),
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

  it("keeps presigned storage uploads on the browser adapter", async () => {
    const browser = vi.fn(async (config) => ({ data: "uploaded", status: 200, statusText: "OK", headers: {}, config }));
    const previous = axios.defaults.adapter;
    axios.defaults.adapter = browser;
    try {
      await client().post("https://storage.example.test/upload", "file", { withCredentials: false });
      expect(browser).toHaveBeenCalledOnce();
      expect(invoke).not.toHaveBeenCalled();
    } finally {
      axios.defaults.adapter = previous;
    }
  });

  it("sends query parameters and JSON through native IPC and decodes the response", async () => {
    const result = await client().post("/api/issues/", { title: "你好" }, { params: { page: 2 } });
    expect(result.data).toEqual({ id: "user-1" });
    const [command, { request }] = invoke.mock.calls[0];
    expect(command).toBe("desktop_api_request");
    expect(request.url).toBe(`${api}/api/issues/?page=2`);
    expect(request.method).toBe("POST");
    expect(new TextDecoder().decode(new Uint8Array(request.body))).toBe('{"title":"你好"}');
    expect(request.headers).toContainEqual(["Content-Type", "application/json"]);
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
    const { request } = invoke.mock.calls[0][1];
    const type = request.headers.find(([name]: [string, string]) => name.toLowerCase() === "content-type")[1];
    expect(type).toMatch(/^multipart\/form-data; boundary=/);
    const body = new TextDecoder().decode(new Uint8Array(request.body));
    expect(body).toContain('filename="test.txt"');
    expect(body).toContain("test contents");
    expect(body).toContain(type.split("boundary=")[1]);
  });

  it("preserves binary downloads", async () => {
    invoke.mockResolvedValueOnce({ ...response(), body: [0, 128, 255] });
    const result = await client().get("/api/export/", { responseType: "arraybuffer" });
    expect(Array.from(new Uint8Array(result.data))).toEqual([0, 128, 255]);
  });

  it("honors cancellation and does not convert it into an auth failure", async () => {
    invoke.mockImplementation(() => new Promise(() => {}));
    const controller = new AbortController();
    const pending = client().get("/api/users/me/", { signal: controller.signal });
    controller.abort();
    await expect(pending).rejects.toMatchObject({ code: "ERR_CANCELED" });
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
