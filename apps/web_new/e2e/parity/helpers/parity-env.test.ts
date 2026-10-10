// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import {
  parityApiUrl,
  parityOracleUrl,
  PARITY_DEFAULT_PORTS,
  PARITY_DEFAULT_PROJECT,
  PARITY_DEFAULT_SEED_FILE,
  resolveParityEnv,
  type ParityPorts,
} from "./parity-env.js";

type Env = Record<string, string | undefined>;

const SERVICE_PORT_VARS: [keyof ParityPorts, string][] = [
  ["oracle", "PARITY_ORACLE_PORT"],
  ["live", "PARITY_LIVE_PORT"],
  ["pg", "PARITY_PG_PORT"],
  ["redis", "PARITY_REDIS_PORT"],
  ["api", "PARITY_API_PORT"],
  ["minio", "PARITY_MINIO_PORT"],
];

describe("resolveParityEnv", () => {
  it("resolves the legacy defaults when nothing is set", () => {
    expect(resolveParityEnv({})).toEqual({
      namespace: null,
      project: PARITY_DEFAULT_PROJECT,
      containerPrefix: PARITY_DEFAULT_PROJECT,
      ports: PARITY_DEFAULT_PORTS,
      seedFileName: PARITY_DEFAULT_SEED_FILE,
    });
  });

  it("treats blank values as unset", () => {
    expect(resolveParityEnv({ PARITY_NS: "  ", PARITY_PROJECT: "", PARITY_API_PORT: " " }).ports.api).toBe(
      PARITY_DEFAULT_PORTS.api
    );
  });

  it("derives project, prefix, ports and seed name from a numeric namespace", () => {
    expect(resolveParityEnv({ PARITY_NS: "257" })).toEqual({
      namespace: "257",
      project: "parity257",
      containerPrefix: "parity257",
      ports: { oracle: 13057, live: 13157, pg: 15457, redis: 16357, api: 18057, minio: 19057 },
      seedFileName: ".seed-257.json",
    });
  });

  it("zero-pads a single-digit namespace", () => {
    const resolved = resolveParityEnv({ PARITY_NS: "6" });
    expect(resolved.project).toBe("parity6");
    expect(resolved.ports).toEqual({
      oracle: 13006,
      live: 13106,
      pg: 15406,
      redis: 16306,
      api: 18006,
      minio: 19006,
    });
  });

  it("lowercases the namespace for project, prefix and seed name", () => {
    const resolved = resolveParityEnv({
      PARITY_NS: "S6",
      PARITY_ORACLE_PORT: "13006",
      PARITY_LIVE_PORT: "13106",
      PARITY_PG_PORT: "15406",
      PARITY_REDIS_PORT: "16306",
      PARITY_API_PORT: "18006",
      PARITY_MINIO_PORT: "19006",
    });
    expect(resolved.namespace).toBe("s6");
    expect(resolved.project).toBe("paritys6");
    expect(resolved.containerPrefix).toBe("paritys6");
    expect(resolved.seedFileName).toBe(".seed-s6.json");
  });

  it("lets an explicit project and prefix win over the namespace", () => {
    const resolved = resolveParityEnv({ PARITY_NS: "257", PARITY_PROJECT: "custom", PARITY_CONTAINER_PREFIX: "other" });
    expect(resolved.project).toBe("custom");
    expect(resolved.containerPrefix).toBe("other");
    // Ports still derive from the namespace; only the names were overridden.
    expect(resolved.ports.oracle).toBe(13057);
  });

  it("lets a single explicit port win while the rest derive", () => {
    const resolved = resolveParityEnv({ PARITY_NS: "257", PARITY_API_PORT: "19999" });
    expect(resolved.ports.api).toBe(19999);
    expect(resolved.ports.oracle).toBe(13057);
  });

  it("keeps the legacy seed name for a project-only override", () => {
    const resolved = resolveParityEnv({ PARITY_NS: "  ", PARITY_PROJECT: "parity257" });
    expect(resolved.namespace).toBeNull();
    expect(resolved.project).toBe("parity257");
    expect(resolved.seedFileName).toBe(PARITY_DEFAULT_SEED_FILE);
  });

  it("rejects namespaces ending in 00 or 19 (they reproduce the default ports)", () => {
    for (const ns of ["19", "119", "100", "0"]) {
      expect(() => resolveParityEnv({ PARITY_NS: ns })).toThrow(/reproduces the default stack's ports/);
    }
  });

  it("rejects malformed namespaces", () => {
    for (const ns of ["has space", "-lead", "_lead", "semi;colon", "a".repeat(33)]) {
      expect(() => resolveParityEnv({ PARITY_NS: ns })).toThrow(/PARITY_NS must be/);
    }
  });

  it("requires explicit ports for a non-numeric namespace", () => {
    expect(() => resolveParityEnv({ PARITY_NS: "s6" })).toThrow(/PARITY_ORACLE_PORT.*explicitly/);
    expect(() => resolveParityEnv({ PARITY_NS: "s6", PARITY_ORACLE_PORT: "13006" })).toThrow(
      /PARITY_LIVE_PORT.*explicitly/
    );
  });

  it("rejects malformed ports", () => {
    for (const port of ["abc", "12a", "0", "65536", "-1", "18019.5"]) {
      expect(() => resolveParityEnv({ PARITY_API_PORT: port })).toThrow(/must be a TCP port/);
    }
  });
});

describe("suite URL helpers", () => {
  it("defaults the oracle and API URLs to the resolved ports", () => {
    expect(parityOracleUrl({})).toBe("http://localhost:13000");
    expect(parityApiUrl({})).toBe("http://localhost:18019");
    expect(parityOracleUrl({ PARITY_NS: "257" })).toBe("http://localhost:13057");
    expect(parityApiUrl({ PARITY_NS: "257" })).toBe("http://localhost:18057");
  });

  it("lets explicit URLs win", () => {
    expect(parityOracleUrl({ PARITY_NS: "257", PARITY_ORACLE_URL: "http://proxy:8080/" })).toBe("http://proxy:8080/");
    expect(parityApiUrl({ PARITY_API_URL: "http://api:9000" })).toBe("http://api:9000");
  });
});

describe("bash conformance (stack/parity-env.sh)", () => {
  const script = join(dirname(fileURLToPath(import.meta.url)), "..", "stack", "parity-env.sh");

  function runBash(env: Env): { status: number; values: Record<string, string> } {
    const clean: Record<string, string | undefined> = {};
    for (const [key, value] of Object.entries(process.env)) {
      if (!key.startsWith("PARITY_")) clean[key] = value;
    }
    const result = spawnSync("bash", [script], { encoding: "utf8", env: { ...clean, ...env } });
    const values: Record<string, string> = {};
    for (const line of String(result.stdout ?? "").split("\n")) {
      const match = /^export ([A-Z_]+)='(.*)'$/.exec(line);
      if (match?.[1] !== undefined && match[2] !== undefined) {
        values[match[1]] = match[2].replaceAll(`'\\''`, "'");
      }
    }
    return { status: result.status ?? 1, values };
  }

  const comparable: Env[] = [
    {},
    { PARITY_NS: "257" },
    { PARITY_NS: "6" },
    { PARITY_NS: " 257 " },
    { PARITY_NS: "257", PARITY_API_PORT: "19999" },
    { PARITY_PROJECT: "parity257" },
    { PARITY_NS: "257", PARITY_PROJECT: "custom", PARITY_CONTAINER_PREFIX: "other" },
    {
      PARITY_NS: "s6",
      PARITY_ORACLE_PORT: "13006",
      PARITY_LIVE_PORT: "13106",
      PARITY_PG_PORT: "15406",
      PARITY_REDIS_PORT: "16306",
      PARITY_API_PORT: "18006",
      PARITY_MINIO_PORT: "19006",
    },
    { PARITY_NS: "257", PARITY_SEED_FILE: "/tmp/custom.json", PARITY_API_URL: "http://api:9000" },
  ];

  for (const env of comparable) {
    it(`agrees with bash for ${JSON.stringify(env)}`, () => {
      const ts = resolveParityEnv(env);
      const bash = runBash(env);
      expect(bash.status).toBe(0);
      expect(bash.values["PARITY_NS"]).toBe(ts.namespace ?? "");
      expect(bash.values["PARITY_PROJECT"]).toBe(ts.project);
      expect(bash.values["PARITY_CONTAINER_PREFIX"]).toBe(ts.containerPrefix);
      for (const [service, name] of SERVICE_PORT_VARS) {
        expect(bash.values[name]).toBe(String(ts.ports[service]));
      }
      expect(bash.values["PARITY_SEED_FILE"]?.split("/").pop()).toBe(
        (env["PARITY_SEED_FILE"] ?? "").trim() === "" ? ts.seedFileName : "custom.json"
      );
      expect(bash.values["PARITY_API_URL"]).toBe(parityApiUrl(env));
      expect(bash.values["PARITY_ORACLE_URL"]).toBe(parityOracleUrl(env));
    });
  }

  const failing: Env[] = [
    { PARITY_NS: "19" },
    { PARITY_NS: "100" },
    { PARITY_NS: "has space" },
    { PARITY_NS: "s6" },
    { PARITY_API_PORT: "abc" },
    { PARITY_API_PORT: "65536" },
  ];

  for (const env of failing) {
    it(`fails in both implementations for ${JSON.stringify(env)}`, () => {
      expect(() => resolveParityEnv(env)).toThrow();
      expect(runBash(env).status).not.toBe(0);
    });
  }
});
