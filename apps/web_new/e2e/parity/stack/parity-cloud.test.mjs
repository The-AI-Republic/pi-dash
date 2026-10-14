// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud-oracle stack selection (NEWFRONT-436). The parity stack serves the
// cloud edition as a selectable oracle (PARITY_ORACLE_EDITION=cloud) with
// the OSS default unchanged. Every test here runs WITHOUT the private
// overlay present — CI has no access to it, so the cloud bring-up must
// fail with setup instructions (never a half-built stack) and every static
// assertion must hold on the committed files alone. (Run: pnpm --filter
// web_new test:parity-stack.)
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const composeFile = join(here, "docker-compose.yml");
const caddyFile = join(here, "Caddyfile");
const upScript = join(here, "parity-up.sh");
const downScript = join(here, "parity-down.sh");
const cloudLib = join(here, "lib", "parity-cloud.sh");
const apiDockerfile = join(here, "Dockerfile.api-cloud");
const oracleDockerfile = join(here, "Dockerfile.oracle-cloud-prod");
const adminCloudDockerfile = join(here, "Dockerfile.oracle-admin-cloud-prod");
const oracleNginxConf = join(here, "nginx-oracle-cloud.conf");
const cloudSeed = join(here, "seed", "seed_cloud.py");
const webNewGitignore = join(here, "..", "..", "..", ".gitignore");

// Minimal indentation parse shared with parity-images.test.mjs: service
// headers sit at 2 spaces. Returns the raw lines of one service block.
function serviceBlock(text, name) {
  const lines = text.split("\n");
  const start = lines.findIndex((line) => line === `  ${name}:`);
  assert.ok(start >= 0, `compose file must define a ${name} service`);
  const block = [];
  for (let i = start + 1; i < lines.length; i += 1) {
    if (lines[i].match(/^  [A-Za-z0-9_-]+:\s*$/)) break;
    if (lines[i].match(/^[A-Za-z]/)) break;
    block.push(lines[i]);
  }
  return block.join("\n");
}

function composeAvailable() {
  try {
    execFileSync("docker", ["compose", "version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function composeConfigServices(profiles) {
  const args = ["compose", "-p", "paritycloudtest", "-f", composeFile];
  for (const profile of profiles) args.push("--profile", profile);
  args.push("config", "--services");
  const env = { ...process.env };
  delete env.CLOUD_OVERLAY_DIR;
  return execFileSync("docker", args, { env, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] })
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

function runBringUp(envExtra) {
  const env = { ...process.env, ...envExtra };
  delete env.PARITY_NS;
  delete env.PARITY_PROJECT;
  delete env.PARITY_CONTAINER_PREFIX;
  try {
    const output = execFileSync("bash", [upScript], { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
    return { status: 0, output };
  } catch (error) {
    const output = [error.stdout, error.stderr].filter(Boolean).join("\n");
    return { status: error.status ?? 1, output };
  }
}

describe("cloud oracle stack (NEWFRONT-436)", () => {
  it("defines api-cloud as the overlaid API on the same ports, scratch settings and healthcheck", () => {
    const text = readFileSync(composeFile, "utf8");
    const api = serviceBlock(text, "api");
    const cloud = serviceBlock(text, "api-cloud");
    assert.ok(cloud.includes("dockerfile: apps/web_new/e2e/parity/stack/Dockerfile.api-cloud"));
    assert.ok(cloud.includes("DJANGO_SETTINGS_MODULE: parity_cloud_scratch_settings"));
    assert.ok(cloud.includes('OPENHUB_APPS_ENABLED: "1"'));
    assert.ok(
      cloud.includes("AIREPUBLIC_SESSION_JWT_SECRET: parity19-fixed-cloud-jwt-secret-not-used-outside-scratch")
    );
    // Same published API port, same database/redis/broker wiring: one stack
    // serves one edition, never both at once.
    for (const needle of [
      '"${PARITY_API_PORT:-18019}:8000"',
      "DATABASE_URL: postgresql://parity19:parity19@pg:5432/pidash",
      "REDIS_URL: redis://redis:6379/0",
    ]) {
      assert.ok(api.includes(needle), `oss api must wire ${needle}`);
      assert.ok(cloud.includes(needle), `api-cloud must wire ${needle}`);
    }
    // The `api` network alias keeps the proxy, the Caddyfile and the live
    // service untouched: api:8000 resolves to whichever api runs.
    assert.ok(cloud.includes("aliases:") && cloud.includes("- api"), "api-cloud must alias the api hostname");
    // Same anon-throttle widening story as OSS (D2): the cloud
    // settings inherit the 30/minute production budget, which
    // saturates minutes into an oracle run, so api-cloud mounts its
    // own scratch settings and runs under them.
    assert.ok(
      cloud.includes("- ./parity_cloud_scratch_settings.py:/code/parity_cloud_scratch_settings.py:ro"),
      "api-cloud must mount the cloud scratch settings"
    );
    // Same liveness probe as OSS: the port binds only after migrate
    // succeeds, so the socket check gates dependents on both editions.
    assert.ok(
      cloud.includes("socket.create_connection((''localhost'', 8000), timeout=5)"),
      "api-cloud must keep the OSS healthcheck"
    );
  });

  it("widens only the anonymous throttle in the cloud scratch settings", () => {
    const text = readFileSync(join(here, "parity_cloud_scratch_settings.py"), "utf8");
    assert.ok(
      text.includes("from pi_dash_cloud.settings.local import *"),
      "cloud scratch settings must layer on the cloud settings"
    );
    assert.ok(text.includes('"anon": "600/minute"'), "cloud scratch settings must widen anon to 600/minute");
  });

  it("defines oracle-cloud-prod as a static nginx oracle with no published ports", () => {
    const text = readFileSync(composeFile, "utf8");
    const cloud = serviceBlock(text, "oracle-cloud-prod");
    assert.ok(cloud.includes("dockerfile: apps/web_new/e2e/parity/stack/Dockerfile.oracle-cloud-prod"));
    assert.ok(!cloud.includes("ports:"), "the cloud oracle serves through the proxy only, like oracle-prod");
  });

  it("keeps all cloud services behind the cloud profile", () => {
    const text = readFileSync(composeFile, "utf8");
    for (const service of ["api-cloud", "oracle-cloud-prod", "oracle-admin-cloud-prod"]) {
      assert.ok(
        serviceBlock(text, service).includes('profiles: ["cloud"]'),
        `${service} must stay out of a bare compose up`
      );
    }
  });

  it("leaves the proxy and Caddyfile untouched by the cloud edition", () => {
    const compose = readFileSync(composeFile, "utf8");
    assert.ok(!compose.includes("api-cloud:8000"), "no cloud upstream in compose; the api alias covers it");
    const caddy = readFileSync(caddyFile, "utf8");
    assert.ok(!caddy.includes("api-cloud"), "no cloud upstream in the Caddyfile; the api alias covers it");
    assert.ok(caddy.includes("{$ORACLE_UPSTREAM:oracle:3000}"), "the oracle stays selectable through ORACLE_UPSTREAM");
  });

  it("builds the cloud API from the stack api image plus one overlay layer", () => {
    const text = readFileSync(apiDockerfile, "utf8");
    assert.ok(text.includes("ARG BASE_API_IMAGE=parity19-api:local"), "namespaced base tag with a legacy default");
    assert.ok(text.includes("FROM $BASE_API_IMAGE"), "layers on the already-built OSS api image");
    assert.ok(
      text.includes(".cloud-staging/ee-overlay/apps/api/ /code/"),
      "overlay api files land on the matching OSS paths"
    );
    assert.ok(
      text.includes(".cloud-staging/pi_dash_cloud /code/pi_dash_cloud"),
      "the cloud package lands beside pi_dash"
    );
    assert.ok(!text.match(/^COPY \. \./m), "no blanket repo copy in the cloud API image");
  });

  it("builds the cloud oracle from OSS sources plus one overlay layer", () => {
    const text = readFileSync(oracleDockerfile, "utf8");
    assert.ok(text.includes("COPY apps/web apps/web"), "the cloud oracle tracks the old app itself");
    assert.ok(
      text.includes(".cloud-staging/ee-overlay/apps/web/ apps/web/"),
      "overlay web files land on the matching OSS paths"
    );
    assert.ok(
      text.includes(".cloud-staging/ee-overlay/packages/ packages/"),
      "overlay package files land on the matching OSS paths"
    );
    assert.ok(text.includes('VITE_OPENHUB_APPS_ENABLED="1"'), "the catalog UI gate bakes on");
    assert.ok(!text.match(/^COPY \. \./m), "no blanket repo copy in the cloud oracle image");
    // The overlay must land before the package builds (replaced package
    // sources compile in) but after the install (which reads manifests
    // only, so overlay edits never invalidate it).
    const installAt = text.indexOf("pnpm install");
    const overlayAt = text.indexOf(".cloud-staging/ee-overlay/apps/web/");
    const buildAt = text.indexOf("tsdown --no-dts");
    assert.ok(
      installAt >= 0 && overlayAt > installAt && buildAt > overlayAt,
      "install, then overlay, then package builds"
    );
    assert.ok(
      text.includes("nginx-oracle-cloud.conf /etc/nginx/nginx.conf"),
      "the cloud oracle serves through its own nginx config"
    );
    assert.ok(
      !text.includes("apps/web/nginx/nginx.conf"),
      "the OSS nginx config 301s prerendered routes to an unreachable host"
    );
  });

  it("defines oracle-admin-cloud-prod as a static nginx oracle on the shared admin alias", () => {
    const text = readFileSync(composeFile, "utf8");
    const admin = serviceBlock(text, "oracle-admin-cloud-prod");
    assert.ok(
      admin.includes("dockerfile: apps/web_new/e2e/parity/stack/Dockerfile.oracle-admin-cloud-prod"),
      "the cloud admin oracle builds from its own Dockerfile"
    );
    assert.ok(!admin.includes("ports:"), "the cloud admin oracle serves through the proxy only, like oracle-prod");
    assert.ok(
      admin.includes("oracle-admin-upstream"),
      "the cloud admin oracle answers the shared alias, so the proxy needs no per-edition admin upstream"
    );
    assert.ok(
      admin.includes("parity19}-oracle-admin-cloud-prod:local"),
      "the image tag interpolates the container prefix with the legacy default"
    );
  });

  it("builds the cloud admin oracle from OSS admin sources plus one overlay layer", () => {
    const text = readFileSync(adminCloudDockerfile, "utf8");
    assert.ok(text.includes("COPY apps/admin apps/admin"), "the cloud admin oracle tracks the old admin app itself");
    assert.ok(
      text.includes(".cloud-staging/ee-overlay/apps/admin/ apps/admin/"),
      "the overlay admin file lands on the matching OSS path"
    );
    assert.ok(!text.match(/^COPY \. \./m), "no blanket repo copy in the cloud admin oracle image");
    // Same layering discipline as the web cloud oracle: install, then
    // overlay, then package builds.
    const installAt = text.indexOf("pnpm install");
    const overlayAt = text.indexOf(".cloud-staging/ee-overlay/apps/admin/");
    const buildAt = text.indexOf("tsdown --no-dts");
    assert.ok(
      installAt >= 0 && overlayAt > installAt && buildAt > overlayAt,
      "install, then overlay, then package builds"
    );
    assert.ok(
      text.includes("apps/admin/nginx/nginx.conf /etc/nginx/nginx.conf"),
      "the cloud admin oracle serves through the admin nginx config"
    );
    assert.ok(text.includes("EXPOSE 3000"), "serves the compose-expected port");
  });

  it("serves prerendered marketing pages directly with an SPA fallback", () => {
    const text = readFileSync(oracleNginxConf, "utf8");
    assert.ok(
      text.includes("try_files $uri/index.html $uri /__spa-fallback.html;"),
      "prerendered file first, then the static path, then the SPA shell"
    );
    assert.ok(text.includes("listen 3000;"), "serves the compose-expected port");
  });

  it("selects the edition in parity-up.sh and refuses anything else", () => {
    const text = readFileSync(upScript, "utf8");
    assert.ok(text.includes('PARITY_ORACLE_EDITION="${PARITY_ORACLE_EDITION:-oss}"'), "oss stays the default");
    assert.ok(text.includes("must be 'oss' or 'cloud'"), "unknown editions fail fast");
    assert.ok(text.includes('API_SERVICE="api-cloud"'), "cloud bring-up targets the overlaid API");
    assert.ok(text.includes("oracle-cloud-prod:3000"), "cloud bring-up points the proxy at the cloud oracle");
    assert.ok(text.includes("has no cloud oracle yet"), "cloud+dev refuses instead of silently serving prod");
    assert.ok(text.includes('exec -T "$API_SERVICE"'), "migrate, bootstrap and seed follow the selected API");
    assert.ok(
      text.includes('WARM_ROUTES="/ /home /login /god-mode/"'),
      "the cloud oracle warms its marketing surface plus the cloud admin entry"
    );
    // The admin oracle follows the edition (NEWFRONT-446): the OSS admin
    // build would serve the wrong entry gate on a cloud stack.
    assert.ok(
      text.includes('ORACLE_ADMIN_SERVICE="oracle-admin-cloud-prod"'),
      "cloud bring-up points the admin mount at the overlaid admin oracle"
    );
    assert.ok(
      text.includes("oracle-admin-cloud-prod:3000"),
      "cloud bring-up points the admin upstream at the overlaid admin oracle"
    );
    assert.ok(
      text.includes("for admin_service in oracle-admin oracle-admin-prod oracle-admin-cloud-prod"),
      "bring-up stops every non-selected admin oracle so the shared alias never splits traffic"
    );
    assert.ok(
      text.includes("up -d --no-deps pg redis mq minio api-cloud"),
      "cloud up names every service and skips OSS deps"
    );
    assert.ok(text.includes("up -d pg redis mq api worker"), "the OSS up line keeps its exact service list");
    // api-cloud layers on the OSS api image, so both build paths (cached
    // and --rebuild) build the base api first; one call would not order
    // them. The trailing newline keeps "build api" from matching the
    // api-cloud service builds further down.
    assert.ok(text.includes("build --no-cache api\n"), "the rebuild path builds the base api before the cloud images");
    assert.ok(text.includes("build api\n"), "the cached path builds the base api before the cloud images");
    // The stack dir sits 5 levels deep, so the repo root climbs five
    // (same off-by-one family as D1).
    assert.ok(
      text.includes('REPO_ROOT="$(cd "$STACK_DIR/../../../../.."'),
      "the repo root climbs five levels to the checkout root"
    );
    assert.ok(
      text.includes('SEED_SCRIPT="$STACK_DIR/seed/seed_cloud.py"'),
      "cloud bring-up seeds through the cloud seed script"
    );
    assert.ok(text.includes('SEED_LABEL="cloud"'), "cloud bring-up labels its seed output");
    assert.ok(
      text.includes("export PARITY_ORACLE_EDITION='cloud'"),
      "the suite env re-exports the cloud edition for specs"
    );
    // The oracle-build gate's anchors survive the edition work.
    assert.ok(text.includes("--rebuild"), "the no-cache escape hatch stays");
    assert.ok(text.includes("warming the oracle"), "the warm-up step stays");
  });

  it("seeds a deterministic cloud identity with the shared seed-fact keys", () => {
    const text = readFileSync(cloudSeed, "utf8");
    assert.ok(text.includes("upsert_dev_identity"), "identity comes from the cloud dev-login helper");
    assert.ok(text.includes('"parity|cloud-seed"'), "the dev-login subject is fixed");
    assert.ok(text.includes('"cloud-oracle@example.com"'), "the dev-login email is fixed");
    for (const key of ["workspaceSlug", "projectId", "issueNames"]) {
      assert.ok(text.includes(`"${key}"`), `seed facts keep the shared ${key} key`);
    }
    assert.ok(text.includes("PARITY_SEED_JSON:"), "facts print on one PARITY_SEED_JSON line");
  });

  it("ignores the overlay staging directory", () => {
    const text = readFileSync(webNewGitignore, "utf8");
    assert.ok(text.includes("e2e/parity/stack/.cloud-staging/"), "staged overlay files must never commit");
  });

  it("drops the overlay staging on teardown, tolerating its absence", () => {
    const text = readFileSync(downScript, "utf8");
    assert.ok(
      text.includes('if [ -f "$STACK_DIR/lib/parity-cloud.sh" ]; then'),
      "teardown guards on the cloud helper surviving"
    );
    assert.ok(
      text.includes("parity_cloud_clean_staging || true"),
      "teardown drops the staging best-effort (OSS stacks never create it)"
    );
    // The helper itself: removes a populated staging tree, and exits 0
    // with nothing staged.
    const stack = mkdtempSync(join(tmpdir(), "parity-cloud-clean-"));
    try {
      mkdirSync(join(stack, ".cloud-staging", "pi_dash_cloud"), { recursive: true });
      writeFileSync(join(stack, ".cloud-staging", "pi_dash_cloud", "marker.py"), "# staged\n");
      const env = { ...process.env, STACK_DIR: stack };
      execFileSync("bash", ["-c", `. "${cloudLib}" && parity_cloud_clean_staging`], {
        env,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      });
      assert.ok(!existsSync(join(stack, ".cloud-staging")), "clean must remove a populated staging tree");
      execFileSync("bash", ["-c", `. "${cloudLib}" && parity_cloud_clean_staging`], {
        env,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      });
      assert.ok(!existsSync(join(stack, ".cloud-staging")), "clean must tolerate an absent staging tree");
    } finally {
      rmSync(stack, { recursive: true, force: true });
    }
  });

  it("refuses an unknown edition without touching docker", () => {
    const { status, output } = runBringUp({ PARITY_ORACLE_EDITION: "bogus" });
    assert.equal(status, 2);
    assert.ok(output.includes("must be 'oss' or 'cloud'"), `refusal must name the valid editions, got: ${output}`);
  });

  it("refuses cloud+dev without touching docker", () => {
    const { status, output } = runBringUp({ PARITY_ORACLE_EDITION: "cloud", PARITY_ORACLE_MODE: "dev" });
    assert.equal(status, 2);
    assert.ok(output.includes("has no cloud oracle yet"), `refusal must explain the mode gap, got: ${output}`);
  });

  it("refuses a cloud bring-up with no overlay checkout", () => {
    const { status, output } = runBringUp({
      PARITY_ORACLE_EDITION: "cloud",
      CLOUD_OVERLAY_DIR: join(tmpdir(), "parity-cloud-missing-overlay"),
    });
    assert.equal(status, 1);
    assert.ok(
      output.includes("git clone") && output.includes("private-pi-dash"),
      `refusal must carry setup instructions, got: ${output}`
    );
    assert.ok(!existsSync(join(here, ".cloud-staging")), "a refused bring-up stages nothing");
  });

  it("resolves and stages a complete overlay checkout", () => {
    const overlay = mkdtempSync(join(tmpdir(), "parity-cloud-overlay-"));
    try {
      for (const sub of [
        "ee-overlay/apps/web",
        "ee-overlay/apps/api",
        "ee-overlay/apps/admin",
        "ee-overlay/packages",
        "pi_dash_cloud",
      ]) {
        mkdirSync(join(overlay, sub), { recursive: true });
      }
      writeFileSync(join(overlay, ".oss-pin"), "pinned-sha\n");
      const script = [
        `. "${cloudLib}"`,
        `resolved="$(parity_cloud_resolve_overlay)"`,
        `test "$resolved" = "${overlay}"`,
        `parity_cloud_stage_overlay "$resolved"`,
      ].join("\n");
      const env = { ...process.env, CLOUD_OVERLAY_DIR: overlay, STACK_DIR: here };
      const output = execFileSync("bash", ["-c", script], { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
      assert.ok(output.includes("cloud overlay staged"), `staging must announce itself, got: ${output}`);
      assert.ok(output.includes("pinned-sha"), "staging must print the pinned OSS SHA for provenance");
      for (const sub of [
        "ee-overlay/apps/web",
        "ee-overlay/apps/api",
        "ee-overlay/apps/admin",
        "ee-overlay/packages",
        "pi_dash_cloud",
      ]) {
        assert.ok(existsSync(join(here, ".cloud-staging", sub)), `staging must carry ${sub}`);
      }
    } finally {
      rmSync(overlay, { recursive: true, force: true });
      rmSync(join(here, ".cloud-staging"), { recursive: true, force: true });
    }
  });

  it("resolves the default overlay sibling with CLOUD_OVERLAY_DIR unset", () => {
    // The documented default is the <repo-root>/../private-pi-dash
    // sibling. The stack dir sits 5 levels deep, so the resolver must
    // climb five: a 4-up climb lands INSIDE the tree (D1). Fake a repo
    // at the real depth so the default resolves without touching this
    // checkout.
    const root = mkdtempSync(join(tmpdir(), "parity-cloud-default-"));
    try {
      const fakeStack = join(root, "repo", "apps", "web_new", "e2e", "parity", "stack");
      mkdirSync(fakeStack, { recursive: true });
      const overlay = join(root, "private-pi-dash");
      for (const sub of [
        "ee-overlay/apps/web",
        "ee-overlay/apps/api",
        "ee-overlay/apps/admin",
        "ee-overlay/packages",
        "pi_dash_cloud",
      ]) {
        mkdirSync(join(overlay, sub), { recursive: true });
      }
      writeFileSync(join(overlay, ".oss-pin"), "pinned-sha\n");
      const script = [`. "${cloudLib}"`, "parity_cloud_resolve_overlay"].join("\n");
      const env = { ...process.env, STACK_DIR: fakeStack };
      delete env.CLOUD_OVERLAY_DIR;
      const resolved = execFileSync("bash", ["-c", script], {
        env,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      }).trim();
      assert.equal(resolved, realpathSync(overlay), "the default must be the outside-tree sibling");
      // A missing sibling refuses with the OUTSIDE-tree path: the
      // message must never steer proprietary code into the tree.
      rmSync(overlay, { recursive: true, force: true });
      let stderr = "";
      try {
        execFileSync("bash", ["-c", script], { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
        assert.fail("the resolver must refuse a missing default sibling");
      } catch (error) {
        stderr = String(error.stderr ?? "");
      }
      // Literal fragments (no path.join: it would normalize the ".."
      // away). The shell always emits forward slashes.
      assert.ok(
        stderr.includes("repo/../private-pi-dash"),
        `refusal must name the outside-tree sibling, got: ${stderr}`
      );
      assert.ok(
        !stderr.includes("apps/../private-pi-dash"),
        `refusal must not name an inside-tree path, got: ${stderr}`
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("refuses an overlay checkout with missing subtrees", () => {
    const overlay = mkdtempSync(join(tmpdir(), "parity-cloud-partial-"));
    try {
      mkdirSync(join(overlay, "ee-overlay", "apps", "web"), { recursive: true });
      const script = [`. "${cloudLib}"`, "parity_cloud_resolve_overlay"].join("\n");
      const env = { ...process.env, CLOUD_OVERLAY_DIR: overlay, STACK_DIR: here };
      let stderr = "";
      let status = 0;
      try {
        execFileSync("bash", ["-c", script], { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
      } catch (error) {
        stderr = String(error.stderr ?? "");
        status = error.status ?? 1;
      }
      assert.equal(status, 1);
      assert.ok(
        stderr.includes("missing 'ee-overlay/apps/api'"),
        `refusal must name the missing subtree, got: ${stderr}`
      );
      assert.ok(
        stderr.includes("missing 'ee-overlay/apps/admin'"),
        `refusal must name the missing admin subtree, got: ${stderr}`
      );
    } finally {
      rmSync(overlay, { recursive: true, force: true });
    }
  });

  it(
    "renders the compose file with no overlay present",
    { skip: composeAvailable() ? false : "docker compose is unavailable" },
    () => {
      // CI has no overlay checkout: config must succeed with the variable
      // unset AND with a bogus value, with and without the cloud profile.
      for (const profiles of [[], ["cloud"]]) {
        for (const overlayValue of [undefined, "/nonexistent-overlay"]) {
          const args = ["compose", "-p", "paritycloudtest", "-f", composeFile];
          for (const profile of profiles) args.push("--profile", profile);
          args.push("config");
          const env = { ...process.env, PARITY_CONTAINER_PREFIX: "paritycloudtest" };
          if (overlayValue === undefined) delete env.CLOUD_OVERLAY_DIR;
          else env.CLOUD_OVERLAY_DIR = overlayValue;
          execFileSync("docker", args, { env, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
        }
      }
    }
  );

  it(
    "keeps cloud services out of a bare compose up",
    { skip: composeAvailable() ? false : "docker compose is unavailable" },
    () => {
      const bare = composeConfigServices([]);
      assert.ok(!bare.includes("api-cloud"), "bare config must not select api-cloud");
      assert.ok(!bare.includes("oracle-cloud-prod"), "bare config must not select oracle-cloud-prod");
      assert.ok(!bare.includes("oracle-admin-cloud-prod"), "bare config must not select oracle-admin-cloud-prod");
      const cloud = composeConfigServices(["cloud"]);
      assert.ok(cloud.includes("api-cloud"), "the cloud profile must select api-cloud");
      assert.ok(cloud.includes("oracle-cloud-prod"), "the cloud profile must select oracle-cloud-prod");
      assert.ok(cloud.includes("oracle-admin-cloud-prod"), "the cloud profile must select oracle-admin-cloud-prod");
    }
  );
});
