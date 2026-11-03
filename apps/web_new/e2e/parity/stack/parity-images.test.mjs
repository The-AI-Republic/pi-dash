// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression test for namespaced parity-stack image tags (run: pnpm
// --filter web_new test:parity-stack). NEWFRONT-435: every namespace's
// locally-built services shared one fixed tag (parity19-api:local and
// friends), so a build in namespace A retagged the image namespace B's
// next recreate pulled, serving the wrong branch's bundle. Each built
// service's image: must now interpolate the container prefix the same
// way container_name does, with the legacy parity19 tag as the default.
// The static assertions always run; the compose-render check additionally
// proves the interpolation resolves per namespace, and skips only when
// docker compose is unavailable.
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const composeFile = join(here, "docker-compose.yml");
const upScript = join(here, "parity-up.sh");

const PREFIX_EXPR = "${PARITY_CONTAINER_PREFIX:-parity19}";
// Every service with a build: section must appear here with a namespaced
// tag; adding a locally-built service means extending this map, not
// pinning a fixed tag.
const EXPECTED_BUILT = new Map([
  ["api", "api"],
  ["oracle", "oracle"],
  ["oracle-prod", "oracle-prod"],
  ["oracle-admin", "oracle-admin"],
  ["oracle-admin-prod", "oracle-admin-prod"],
  ["worker", "api"],
  ["live", "live"],
  // Cloud edition (NEWFRONT-436): same rule — the overlay images are
  // namespaced like every other locally-built tag.
  ["api-cloud", "api-cloud"],
  ["oracle-cloud-prod", "oracle-cloud-prod"],
  // Cloud admin oracle (NEWFRONT-446): same rule — the overlaid admin
  // image is namespaced like every other locally-built tag.
  ["oracle-admin-cloud-prod", "oracle-admin-cloud-prod"],
]);

// Minimal indentation parse: service headers sit at 2 spaces, their keys
// (build:/image:) at 4. Full YAML parsing would need a dependency for a
// shape this regular; the compose-render test below proves real parsing.
function builtServices(text) {
  const found = new Map();
  let service = null;
  let hasBuild = false;
  let image = null;
  const flush = () => {
    if (service !== null && hasBuild) found.set(service, image);
  };
  for (const line of text.split("\n")) {
    const header = line.match(/^  ([A-Za-z0-9_-]+):\s*$/);
    if (header) {
      flush();
      service = header[1];
      hasBuild = false;
      image = null;
      continue;
    }
    if (service !== null && line.startsWith("    build:")) hasBuild = true;
    if (service !== null && line.startsWith("    image:")) {
      image = line.slice("    image:".length).trim();
    }
  }
  flush();
  return found;
}

function composeAvailable() {
  try {
    execFileSync("docker", ["compose", "version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function renderedLocalImages(prefix) {
  const env = { ...process.env };
  delete env.COMPOSE_PROFILES;
  delete env.PARITY_NS;
  delete env.PARITY_PROJECT;
  if (prefix === null) {
    delete env.PARITY_CONTAINER_PREFIX;
  } else {
    env.PARITY_CONTAINER_PREFIX = prefix;
  }
  const out = execFileSync(
    "docker",
    [
      "compose",
      "-p",
      "parityimagestest",
      "-f",
      composeFile,
      "--profile",
      "oracle-prod",
      "--profile",
      "full",
      "--profile",
      "cloud",
      "config",
    ],
    { env, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }
  );
  const images = new Set();
  for (const line of out.split("\n")) {
    const m = line.match(/^\s*image:\s*(\S+)\s*$/);
    if (m && m[1].endsWith(":local")) images.add(m[1]);
  }
  return images;
}

describe("parity stack image tags (NEWFRONT-435)", () => {
  it("namespaces every locally-built service tag by container prefix", () => {
    const text = readFileSync(composeFile, "utf8");
    const found = builtServices(text);
    assert.deepEqual(
      [...found.keys()].sort(),
      [...EXPECTED_BUILT.keys()].sort(),
      "built-service set changed; extend EXPECTED_BUILT with a namespaced tag"
    );
    for (const [service, tag] of EXPECTED_BUILT) {
      assert.equal(
        found.get(service),
        `${PREFIX_EXPR}-${tag}:local`,
        `image: for ${service} must interpolate the container prefix`
      );
    }
  });

  it("pins no fixed parity19- image tag anywhere in the compose file", () => {
    const text = readFileSync(composeFile, "utf8");
    for (const line of text.split("\n")) {
      assert.ok(!line.match(/^\s*image:\s*parity19-/), `fixed image tag must go through the prefix: ${line.trim()}`);
    }
  });

  it("names the resolved tag (not a fixed one) in parity-up.sh logs", () => {
    const text = readFileSync(upScript, "utf8");
    assert.ok(!text.includes("parity19-api"), "parity-up.sh must not name a fixed image tag");
    // The edition-aware log line interpolates the prefix and the selected
    // api service (NEWFRONT-436); OSS still renders exactly the old tag.
    assert.ok(text.includes(`${PREFIX_EXPR}-$API_SERVICE`), "parity-up.sh logs must interpolate the resolved api tag");
  });

  it(
    "renders distinct tags per namespace with a legacy default",
    { skip: composeAvailable() ? false : "docker compose is unavailable" },
    () => {
      const a = "parityimagestesta";
      const b = "parityimagestestb";
      const imagesA = renderedLocalImages(a);
      const imagesB = renderedLocalImages(b);
      const expectedA = new Set([...EXPECTED_BUILT.values()].map((tag) => `${a}-${tag}:local`));
      assert.deepEqual(imagesA, expectedA, "rendered tags must carry the namespace prefix");
      assert.ok(
        [...imagesA].every((image) => !imagesB.has(image)),
        "two namespaces must not share an image tag"
      );
      assert.deepEqual(
        renderedLocalImages(null),
        new Set([...EXPECTED_BUILT.values()].map((tag) => `parity19-${tag}:local`)),
        "unset prefix must render exactly the legacy tags"
      );
    }
  );
});
