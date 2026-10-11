// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-checkout parity-stack namespacing (NEWFRONT-132). One setting,
// PARITY_NS, derives the compose project, container prefix, published
// ports, seed-file name and suite URLs, so parallel oracle runs on one
// host stop sharing a database. Explicit PARITY_* settings always win;
// with no namespace set every default is exactly the legacy one. The
// same algorithm lives in stack/parity-env.sh for the shell scripts;
// parity-env.test.ts holds the two implementations to one behavior.
export interface ParityPorts {
  oracle: number;
  live: number;
  pg: number;
  redis: number;
  api: number;
  minio: number;
}

export interface ParityEnv {
  /** Lowercased namespace, or null when no PARITY_NS is set. */
  namespace: string | null;
  project: string;
  containerPrefix: string;
  ports: ParityPorts;
  /** ".seed.json", or ".seed-<ns>.json" under a namespace. */
  seedFileName: string;
}

export const PARITY_DEFAULT_PROJECT = "parity19";

export const PARITY_DEFAULT_PORTS: ParityPorts = {
  oracle: 13000,
  live: 13001,
  pg: 15419,
  redis: 16319,
  api: 18019,
  minio: 19019,
};

export const PARITY_DEFAULT_SEED_FILE = ".seed.json";

/** A namespace is a short token: letters, digits, `_`, `-`, starting alnum. */
const NAMESPACE_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_-]*$/;
const MAX_NAMESPACE_LENGTH = 32;

/** Ports derive from per-service bases plus the namespace's last two digits. */
const PORT_BASES: ParityPorts = {
  oracle: 13000,
  live: 13100,
  pg: 15400,
  redis: 16300,
  api: 18000,
  minio: 19000,
};

const PORT_VARS: Record<keyof ParityPorts, string> = {
  oracle: "PARITY_ORACLE_PORT",
  live: "PARITY_LIVE_PORT",
  pg: "PARITY_PG_PORT",
  redis: "PARITY_REDIS_PORT",
  api: "PARITY_API_PORT",
  minio: "PARITY_MINIO_PORT",
};

type Env = Record<string, string | undefined>;

/** Blank counts as unset everywhere in this module. */
function pick(env: Env, name: string): string | null {
  const raw = env[name];
  if (raw === undefined) return null;
  const trimmed = raw.trim();
  return trimmed === "" ? null : trimmed;
}

function parsePort(service: keyof ParityPorts, raw: string): number {
  if (!/^[0-9]+$/.test(raw)) {
    throw new Error(`[parity] ${PORT_VARS[service]} must be a TCP port (1-65535), got ${JSON.stringify(raw)}.`);
  }
  const port = Number.parseInt(raw, 10);
  if (port < 1 || port > 65535) {
    throw new Error(`[parity] ${PORT_VARS[service]} must be a TCP port (1-65535), got ${JSON.stringify(raw)}.`);
  }
  return port;
}

function checkNamespace(ns: string): string {
  if (ns.length > MAX_NAMESPACE_LENGTH || !NAMESPACE_PATTERN.test(ns)) {
    throw new Error(
      `[parity] PARITY_NS must be 1-${MAX_NAMESPACE_LENGTH} chars of letters, digits, "_" or "-", starting alnum; got ${JSON.stringify(ns)}.`
    );
  }
  return ns.toLowerCase();
}

/**
 * The two-digit port suffix for a numeric namespace (last two digits,
 * zero-padded), or null for a non-numeric one. Suffixes 00 and 19 are
 * rejected: they reproduce the default stack's own ports.
 */
function portSuffix(ns: string): number | null {
  if (!/^[0-9]+$/.test(ns)) return null;
  const digits = ns.slice(-2).padStart(2, "0");
  if (digits === "00" || digits === "19") {
    throw new Error(
      `[parity] PARITY_NS=${JSON.stringify(ns)} ends in "${digits}", which reproduces the default stack's ports; pick a namespace with a different last-two-digits, or set the PARITY_*_PORT variables explicitly.`
    );
  }
  return Number.parseInt(digits, 10);
}

/** Resolve the full stack identity from the environment. Throws on invalid input. */
export function resolveParityEnv(env: Env): ParityEnv {
  const rawNs = pick(env, "PARITY_NS");
  const namespace = rawNs === null ? null : checkNamespace(rawNs);
  const project = pick(env, "PARITY_PROJECT") ?? (namespace === null ? PARITY_DEFAULT_PROJECT : `parity${namespace}`);
  const containerPrefix = pick(env, "PARITY_CONTAINER_PREFIX") ?? project;
  const suffix = namespace === null ? null : portSuffix(namespace);

  const ports = {} as ParityPorts;
  (Object.keys(PORT_BASES) as (keyof ParityPorts)[]).forEach((service) => {
    const explicit = pick(env, PORT_VARS[service]);
    if (explicit !== null) {
      ports[service] = parsePort(service, explicit);
    } else if (suffix !== null) {
      ports[service] = PORT_BASES[service] + suffix;
    } else if (namespace !== null) {
      throw new Error(
        `[parity] PARITY_NS=${JSON.stringify(namespace)} is not numeric, so ports cannot be derived; set ${PORT_VARS[service]} explicitly (or use a numeric namespace).`
      );
    } else {
      ports[service] = PARITY_DEFAULT_PORTS[service];
    }
  });

  return {
    namespace,
    project,
    containerPrefix,
    ports,
    seedFileName: namespace === null ? PARITY_DEFAULT_SEED_FILE : `.seed-${namespace}.json`,
  };
}

/** The oracle base URL: explicit PARITY_ORACLE_URL, else derived from the namespace. */
export function parityOracleUrl(env: Env): string {
  return pick(env, "PARITY_ORACLE_URL") ?? `http://localhost:${resolveParityEnv(env).ports.oracle}`;
}

/** The API base URL: explicit PARITY_API_URL, else derived from the namespace. */
export function parityApiUrl(env: Env): string {
  return pick(env, "PARITY_API_URL") ?? `http://localhost:${resolveParityEnv(env).ports.api}`;
}
