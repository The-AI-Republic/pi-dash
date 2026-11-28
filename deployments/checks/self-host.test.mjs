// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-host smoke checks. A fresh community install pulls every
// compose image, reads variables.env into the services, and relies on the
// web_new nginx answering 404 for a missing bundle so the proxy can fall
// back to the old app. None of that is exercised by the app test suites, so
// these checks read the shipped files directly.
//
// Run: node --test deployments/checks/*.test.mjs
import { strict as assert } from "node:assert";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const root = resolve(join(dirname(fileURLToPath(import.meta.url)), "..", ".."));

// Images that can no longer be pulled anonymously. minio/minio stopped being
// published on Docker Hub, and quay.io/minio/minio now rejects anonymous
// pulls; a compose file naming either fails `docker compose up`.
const GONE_IMAGES = new Set(["minio/minio", "docker.io/minio/minio", "quay.io/minio/minio"]);

const SKIP_DIRS = new Set([".git", "node_modules", "target", "dist", "build", ".turbo", ".next"]);

/** Every docker compose file in the repo, as absolute paths. */
function composeFiles(dir = root, found = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name) && !entry.name.startsWith(".pidash")) composeFiles(join(dir, entry.name), found);
    } else if (/^(docker-)?compose[\w.-]*\.ya?ml$/.test(entry.name)) {
      found.push(join(dir, entry.name));
    }
  }
  return found;
}

/** `image:` values of a compose file, with their 1-based line numbers. */
function imagesOf(text) {
  const images = [];
  text.split("\n").forEach((line, index) => {
    const match = /^\s*image:\s*["']?([^"'\s#]+)/.exec(line);
    if (match !== null) images.push({ line: index + 1, image: match[1] });
  });
  return images;
}

/** Repository part of an image reference (no tag, no digest). */
function imageRepository(image) {
  const withoutDigest = image.split("@")[0];
  const lastSlash = withoutDigest.lastIndexOf("/");
  const colon = withoutDigest.indexOf(":", lastSlash + 1);
  return colon === -1 ? withoutDigest : withoutDigest.slice(0, colon);
}

/** Tag of an image reference, or null when it has none. */
function imageTag(image) {
  const withoutDigest = image.split("@")[0];
  const lastSlash = withoutDigest.lastIndexOf("/");
  const colon = withoutDigest.indexOf(":", lastSlash + 1);
  return colon === -1 ? null : withoutDigest.slice(colon + 1);
}

/** KEY=VALUE pairs of an env file, skipping blanks and comments. */
function parseEnvFile(text) {
  const pairs = [];
  text.split("\n").forEach((line, index) => {
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) return;
    const at = line.indexOf("=");
    if (at === -1) return;
    pairs.push({ line: index + 1, key: line.slice(0, at).trim(), value: line.slice(at + 1) });
  });
  return pairs;
}

/** True when a value has whitespace outside a single- or double-quoted string. */
function hasUnquotedWhitespace(value) {
  const quoted = /^(["']).*\1$/.exec(value);
  if (quoted !== null) return false;
  return /\s/.test(value);
}

/** `${KEY:-default}` defaults declared in a compose file. */
function composeDefaults(text) {
  const defaults = new Map();
  for (const match of text.matchAll(/\$\{([A-Z0-9_]+):-([^}]*)\}/g)) {
    if (!defaults.has(match[1])) defaults.set(match[1], match[2]);
  }
  return defaults;
}

describe("helpers", () => {
  it("splits image references into repository and tag", () => {
    assert.equal(imageRepository("minio/minio"), "minio/minio");
    assert.equal(imageRepository("minio/minio:latest"), "minio/minio");
    assert.equal(imageRepository("quay.io/minio/minio:RELEASE.x"), "quay.io/minio/minio");
    assert.equal(imageRepository("localhost:5000/minio/minio@sha256:ab"), "localhost:5000/minio/minio");
    assert.equal(imageTag("pgsty/minio:RELEASE.1@sha256:ab"), "RELEASE.1");
    assert.equal(imageTag("minio/minio"), null);
  });

  it("flags unquoted whitespace only", () => {
    assert.equal(hasUnquotedWhitespace("pi dash"), true);
    assert.equal(hasUnquotedWhitespace("pi-dash"), false);
    assert.equal(hasUnquotedWhitespace('"Pi Dash <a@b>"'), false);
    assert.equal(hasUnquotedWhitespace("trailing "), true);
  });

  it("reads image lines from compose text", () => {
    assert.deepEqual(imagesOf("services:\n  a:\n    image: 'x/y:1' # pinned\n"), [{ line: 3, image: "x/y:1" }]);
  });
});

describe("compose images", () => {
  const files = composeFiles();

  it("finds the community compose file", () => {
    assert.ok(
      files.some((file) => relative(root, file) === join("deployments", "cli", "community", "docker-compose.yml")),
      "deployments/cli/community/docker-compose.yml was not discovered"
    );
  });

  it("never references an image that can no longer be pulled", () => {
    const offenders = [];
    for (const file of files) {
      for (const { line, image } of imagesOf(readFileSync(file, "utf8"))) {
        if (GONE_IMAGES.has(imageRepository(image))) offenders.push(`${relative(root, file)}:${line}: ${image}`);
      }
    }
    assert.deepEqual(offenders, [], `compose files reference unpullable images:\n${offenders.join("\n")}`);
  });

  it("pins the community object storage image to a release instead of floating on latest", () => {
    const text = readFileSync(join(root, "deployments", "cli", "community", "docker-compose.yml"), "utf8");
    const block = /\n {2}pi-dash-minio:\n((?: {4}.*\n|\n)*)/.exec(text);
    assert.ok(block !== null, "pi-dash-minio service not found");
    const [minio] = imagesOf(block[1]);
    assert.ok(minio !== undefined, "pi-dash-minio has no image");
    const tag = imageTag(minio.image);
    assert.ok(tag !== null && tag !== "latest", `pi-dash-minio image is not pinned: ${minio.image}`);
    assert.match(minio.image, /@sha256:[0-9a-f]{64}$/, "pi-dash-minio image should carry its digest");
  });
});

describe("third-party object storage image", () => {
  // The official MinIO image can no longer be pulled anonymously; the
  // bundled object store (service pi-dash-minio) runs PGSTY's maintained
  // fork. The self-host README records the trust decision and the exact
  // reference; both must move together on a re-pin.
  const REPOSITORY = "pgsty/silo";
  // SN-2026-020 (critical): single-pool S3 DeleteObjects could delete
  // objects in sibling buckets. Fixed in RELEASE.2026-11-12T00-00-00Z.
  const FIRST_PATCHED_RELEASE = "RELEASE.2026-11-12T00-00-00Z";

  const services = composeFiles().flatMap((file) => {
    const text = readFileSync(file, "utf8");
    const block = /\n {2}pi-dash-minio:\n((?: {4}.*\n|\n)*)/.exec(text);
    if (block === null) return [];
    const offset = text.slice(0, block.index).split("\n").length;
    return imagesOf(block[1]).map(({ line, image }) => ({
      where: `${relative(root, file)}:${offset + line}`,
      image,
      block: block[1],
    }));
  });

  it("is the same pinned reference, by tag and digest, in every compose file", () => {
    assert.ok(services.length >= 3, services.map((s) => s.where).join("\n"));
    for (const { where, image } of services) {
      assert.match(image, /^pgsty\/silo:RELEASE\.[^@]+@sha256:[0-9a-f]{64}$/, `${where}: ${image}`);
    }
    assert.equal(
      new Set(services.map(({ image }) => image)).size,
      1,
      services.map((s) => `${s.where}: ${s.image}`).join("\n")
    );
  });

  it("is a release with the SN-2026-020 fix", () => {
    for (const { where, image } of services) {
      const tag = imageTag(image) ?? "";
      assert.ok(tag >= FIRST_PATCHED_RELEASE, `${where}: ${tag} predates ${FIRST_PATCHED_RELEASE}`);
    }
  });

  it("never calls a `minio` binary, which the image does not ship", () => {
    // The image's entrypoint maps `server ...` and `minio ...` to `silo`,
    // but a shell entrypoint override runs binaries by name: `silo`, and
    // `mc` (a link to `mcli`).
    for (const { where, block } of services) {
      assert.doesNotMatch(block, /(^|[\s;&|(])minio server/m, `${where}: calls \`minio server\``);
    }
  });

  it("is recorded, with the same digest, in the self-host README", () => {
    const readme = readFileSync(join(root, "deployments", "cli", "community", "README.md"), "utf8");
    assert.ok(readme.includes(`\`${services[0].image}\``), `README does not record ${services[0].image}`);
    for (const user of new Set(services.map(({ where }) => where.split(":")[0]))) {
      assert.ok(readme.includes(`\`${user}\``), `README's "Used by" row does not list ${user}`);
    }
    assert.ok(readme.includes("SN-2026-020"), "README does not record why this release is the minimum");
  });

  it("is what the CI registry check resolves", () => {
    const workflow = readFileSync(join(root, ".github", "workflows", "self-host-checks.yml"), "utf8");
    assert.ok(workflow.includes(REPOSITORY), `self-host-checks.yml does not resolve ${REPOSITORY}`);
  });
});

describe("variables.env templates", () => {
  const templates = [
    join(root, "deployments", "cli", "community", "variables.env"),
    join(root, "deployments", "aio", "community", "variables.env"),
  ];

  for (const template of templates) {
    it(`${relative(root, template)} has no values with unquoted spaces`, () => {
      const offenders = parseEnvFile(readFileSync(template, "utf8"))
        .filter(({ value }) => hasUnquotedWhitespace(value))
        .map(({ line, key, value }) => `${line}: ${key}=${value}`);
      assert.deepEqual(offenders, [], `unquoted spaces break compose/URL interpolation:\n${offenders.join("\n")}`);
    });
  }

  it("agrees with the community compose file's own defaults", () => {
    const community = join(root, "deployments", "cli", "community");
    const defaults = composeDefaults(readFileSync(join(community, "docker-compose.yml"), "utf8"));
    const mismatches = parseEnvFile(readFileSync(join(community, "variables.env"), "utf8"))
      // Empty values defer to the compose default; interpolated ones are
      // derived from another variable on purpose (WEB_URL from APP_DOMAIN).
      .filter(({ key, value }) => value !== "" && !value.includes("${") && defaults.has(key))
      .filter(({ key, value }) => defaults.get(key) !== value)
      .map(({ line, key, value }) => `${line}: ${key}=${value} (compose default: ${defaults.get(key)})`);
    assert.deepEqual(mismatches, [], `variables.env disagrees with docker-compose.yml:\n${mismatches.join("\n")}`);
  });
});

describe("web_new nginx", () => {
  it("answers 404 for a missing /assets/ file instead of the SPA shell", () => {
    // The CE proxy sends /assets/* to web_new first and retries the old app
    // on a 404; an index.html fallback here would hide every old-app bundle.
    const conf = readFileSync(join(root, "apps", "web_new", "nginx", "nginx.conf"), "utf8");
    const block = /location\s+\/assets\/\s*\{([^}]*)\}/.exec(conf);
    assert.ok(block !== null, "web_new nginx has no `location /assets/` block");
    assert.match(block[1], /try_files\s+\$uri\s+=404;/);
  });

  it("serves .webmanifest as application/manifest+json", () => {
    // nginx's stock mime.types has no webmanifest entry, so web_new's
    // manifests would go out as application/octet-stream.
    const conf = readFileSync(join(root, "apps", "web_new", "nginx", "nginx.conf"), "utf8");
    const http = /^http\s*\{([\s\S]*?)^\s*server\s*\{/m.exec(conf);
    assert.ok(http !== null, "web_new nginx has no http block before its server block");
    assert.match(http[1], /include\s+mime\.types;/);
    assert.match(http[1], /types\s*\{[^}]*application\/manifest\+json\s+webmanifest;[^}]*\}/);
  });

  /** The `location ~ ^/(dir|...)/...` block for web_new's public directories. */
  function publicDirLocation(conf) {
    const block = /location\s+~\s+(\^\/\(([^)]*)\)\/\S*)\s*\{([^}]*)\}/.exec(conf);
    assert.ok(block !== null, "web_new nginx has no `location ~ ^/(dir|...)/` block for its public directories");
    return { regexp: new RegExp(block[1]), dirs: block[2].split("|"), body: block[3] };
  }

  it("answers 404 for a missing file under every public directory", () => {
    // The proxy sends web_new's public directories (illustrations/, logos/,
    // ...) here first, with the same 404 retry of the old app as /assets/.
    const conf = readFileSync(join(root, "apps", "web_new", "nginx", "nginx.conf"), "utf8");
    const location = publicDirLocation(conf);
    assert.match(location.body, /try_files\s+\$uri\s+=404;/);
    const dirs = readdirSync(join(root, "apps", "web_new", "public"), { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && !entry.name.startsWith("."))
      .map((entry) => entry.name)
      .toSorted();
    assert.deepEqual(
      location.dirs.toSorted(),
      dirs,
      "nginx public-directory list is out of step with apps/web_new/public/"
    );
    for (const file of ["/logos/github.svg", "/illustrations/not-found.svg", "/empty-state/a/b.webp"]) {
      assert.ok(location.regexp.test(file), `${file} should be served as a file (or 404)`);
    }
  });

  it("still serves the SPA shell for a workspace page under a public directory name", () => {
    // Workspace slugs are not reserved against public/ names: /logos/projects
    // is a page of workspace "logos" and must reach the SPA fallback.
    const conf = readFileSync(join(root, "apps", "web_new", "nginx", "nginx.conf"), "utf8");
    const location = publicDirLocation(conf);
    for (const page of [
      "/logos/projects",
      "/illustrations/projects/6f1e/issues",
      "/empty-state/",
      "/logos/active-cycles/",
    ]) {
      assert.ok(!location.regexp.test(page), `${page} would 404 instead of loading the SPA`);
    }
  });
});
