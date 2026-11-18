// Tests for the pidash skill package: the installer, and that the skill's
// documentation only names `pidash` commands the CLI source actually defines.
//
// Run with `npm test` from `skill/` (Node's built-in test runner, no deps).

import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SKILL_PKG_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SKILL_DIR = join(SKILL_PKG_DIR, "pidash");
const INSTALLER = join(SKILL_PKG_DIR, "install.mjs");
const CLI_SRC_DIR = resolve(SKILL_PKG_DIR, "..", "runner", "src", "cli");
const SKILL_DOCS = [
  join(SKILL_DIR, "SKILL.md"),
  join(SKILL_DIR, "references", "pidash-workflows.md"),
  join(SKILL_DIR, "references", "pidash-mcp.md"),
];

function runInstaller(args, homes) {
  return spawnSync(process.execPath, [INSTALLER, ...args], {
    encoding: "utf8",
    env: { ...process.env, CLAUDE_HOME: homes.claude, CODEX_HOME: homes.codex },
  });
}

function withTempHomes(fn) {
  const root = mkdtempSync(join(tmpdir(), "pidash-skill-test-"));
  try {
    return fn({ claude: join(root, "claude"), codex: join(root, "codex") });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("--all installs the skill into both agents", () => {
  withTempHomes((homes) => {
    const result = runInstaller(["--all"], homes);

    assert.equal(result.status, 0, result.stderr);
    for (const home of [homes.claude, homes.codex]) {
      const installed = join(home, "skills", "pidash");
      assert.equal(
        readFileSync(join(installed, "SKILL.md"), "utf8"),
        readFileSync(join(SKILL_DIR, "SKILL.md"), "utf8")
      );
      assert.ok(existsSync(join(installed, "references", "pidash-workflows.md")));
    }
  });
});

test("the Codex manifest is installed for Codex only", () => {
  withTempHomes((homes) => {
    runInstaller(["--all"], homes);

    assert.ok(existsSync(join(homes.codex, "skills", "pidash", "agents", "openai.yaml")));
    assert.ok(!existsSync(join(homes.claude, "skills", "pidash", "agents")));
  });
});

test("a single-target flag leaves the other agent untouched", () => {
  withTempHomes((homes) => {
    const result = runInstaller(["--claude-code"], homes);

    assert.equal(result.status, 0, result.stderr);
    assert.ok(existsSync(join(homes.claude, "skills", "pidash", "SKILL.md")));
    assert.ok(!existsSync(homes.codex));
  });
});

test("reinstalling replaces stale files and removes the legacy pi-dash folder", () => {
  withTempHomes((homes) => {
    const skillsDir = join(homes.claude, "skills");
    const staleFile = join(skillsDir, "pidash", "stale.md");
    const legacyDir = join(skillsDir, "pi-dash");
    mkdirSync(dirname(staleFile), { recursive: true });
    writeFileSync(staleFile, "left over from an older install");
    mkdirSync(legacyDir, { recursive: true });

    const result = runInstaller(["--claude-code"], homes);

    assert.equal(result.status, 0, result.stderr);
    assert.ok(!existsSync(staleFile));
    assert.ok(!existsSync(legacyDir));
    assert.ok(existsSync(join(skillsDir, "pidash", "SKILL.md")));
  });
});

test("an unknown flag fails without installing anything", () => {
  withTempHomes((homes) => {
    const result = runInstaller(["--nope"], homes);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /unknown flag/);
    assert.ok(!existsSync(homes.claude));
  });
});

test("no target flag on a non-interactive stdin fails with guidance", () => {
  withTempHomes((homes) => {
    const result = runInstaller([], homes);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /--all, --claude-code, or --codex/);
  });
});

test("SKILL.md frontmatter names the skill and describes when to use it", () => {
  const text = readFileSync(join(SKILL_DIR, "SKILL.md"), "utf8");
  const frontmatter = text.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/)?.[1] ?? "";

  assert.match(frontmatter, /^name: pidash$/m);
  const description = frontmatter.match(/^description: (.+)$/m)?.[1] ?? "";
  assert.ok(description.length > 0, "description is missing");
  assert.ok(description.length <= 1024, `description is ${description.length} chars; keep it <= 1024`);
});

test("the package ships the skill folder", () => {
  const pkg = JSON.parse(readFileSync(join(SKILL_PKG_DIR, "package.json"), "utf8"));

  assert.ok(pkg.files.includes("pidash"));
  assert.ok(pkg.files.includes("install.mjs"));
  assert.equal(pkg.dependencies, undefined, "the installer must stay dependency-free");
});

// --- Skill docs vs. the CLI source -----------------------------------------

// Top-level command -> the clap source file that defines its subcommands.
// `null` means the command takes no subcommand worth checking.
const COMMAND_SOURCES = {
  issue: "issue.rs",
  comment: "comment.rs",
  label: "label.rs",
  page: "page.rs",
  state: "state.rs",
  project: "project.rs",
  context: "context.rs",
  workpad: "workpad.rs",
  workspace: "workspace.rs",
  scheduler: "scheduler.rs",
  run: "run_cmd.rs",
  config: "config_cmd.rs",
  auth: null,
  ai: null,
  update: null,
};

const toVariant = (kebab) =>
  kebab
    .split("-")
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join("");

function definesVariant(sourceFile, variant) {
  const source = readFileSync(join(CLI_SRC_DIR, sourceFile), "utf8");
  return new RegExp(`^\\s+${variant}\\s*[({,]`, "m").test(source);
}

// Every `pidash <command> [<subcommand>]` mention in the skill docs.
function mentionedCommands() {
  const mentions = new Set();
  for (const doc of SKILL_DOCS) {
    const text = readFileSync(doc, "utf8");
    for (const match of text.matchAll(/\bpidash ([a-z][a-z-]*)(?: ([a-z][a-z-]*))?/g)) {
      const [, command, maybeSub] = match;
      if (!(command in COMMAND_SOURCES)) {
        mentions.add(command);
        continue;
      }
      const hasSubcommands = COMMAND_SOURCES[command] !== null;
      mentions.add(hasSubcommands && maybeSub ? `${command} ${maybeSub}` : command);
    }
  }
  return [...mentions].toSorted();
}

test("every pidash command the skill mentions exists in the CLI source", () => {
  const unknown = [];

  for (const mention of mentionedCommands()) {
    const [command, sub] = mention.split(" ");
    if (!(command in COMMAND_SOURCES)) {
      unknown.push(`pidash ${mention} (unknown top-level command)`);
      continue;
    }
    const sourceFile = COMMAND_SOURCES[command];
    if (sourceFile === null || sub === undefined) continue;
    if (!definesVariant(sourceFile, toVariant(sub))) {
      unknown.push(`pidash ${mention} (no ${toVariant(sub)} in runner/src/cli/${sourceFile})`);
    }
  }

  assert.deepEqual(unknown, []);
});

test("the command check actually finds commands to check", () => {
  const mentions = mentionedCommands();

  assert.ok(mentions.includes("issue create"));
  assert.ok(mentions.includes("comment add"));
  assert.ok(mentions.length > 20, `only found ${mentions.length} command mentions`);
});

// Every `--flag` the skill docs mention must be declared somewhere in the CLI
// source: either as an explicit `long = "flag"` or as a clap field named
// `flag` in snake_case. Coarser than per-command checking, but it catches a
// flag that was renamed or removed.
function declaredFlags() {
  const flags = new Set(["help", "version"]);
  for (const file of readdirSync(CLI_SRC_DIR, { recursive: true })) {
    if (!String(file).endsWith(".rs")) continue;
    const source = readFileSync(join(CLI_SRC_DIR, String(file)), "utf8");
    for (const match of source.matchAll(/long\s*=\s*"([a-z][a-z0-9-]*)"/g)) flags.add(match[1]);
    for (const match of source.matchAll(/^\s+(?:pub(?:\([a-z]+\))? )?([a-z][a-z0-9_]*):/gm)) {
      flags.add(match[1].replaceAll("_", "-"));
    }
  }
  return flags;
}

test("every flag the skill mentions is declared in the CLI source", () => {
  const declared = declaredFlags();
  const mentioned = new Set();
  for (const doc of SKILL_DOCS) {
    for (const match of readFileSync(doc, "utf8").matchAll(/(?<![\w-])--([a-z][a-z0-9-]*)/g)) {
      mentioned.add(match[1]);
    }
  }

  const undeclared = [...mentioned].filter((flag) => !declared.has(flag)).toSorted();

  assert.ok(mentioned.size > 30, `only found ${mentioned.size} flag mentions`);
  assert.deepEqual(undeclared, []);
});

test("every reference file SKILL.md points to exists and is installed", () => {
  const skillText = readFileSync(join(SKILL_DIR, "SKILL.md"), "utf8");
  const referenced = [...new Set([...skillText.matchAll(/references\/[a-z0-9-]+\.md/g)].map((m) => m[0]))];

  assert.ok(referenced.includes("references/pidash-workflows.md"));
  assert.ok(referenced.includes("references/pidash-mcp.md"));
  withTempHomes((homes) => {
    runInstaller(["--all"], homes);
    for (const reference of referenced) {
      assert.ok(existsSync(join(SKILL_DIR, reference)), `${reference} is missing from the skill`);
      for (const home of [homes.claude, homes.codex]) {
        assert.ok(existsSync(join(home, "skills", "pidash", reference)), `${reference} was not installed`);
      }
    }
  });
});

test("the MCP reference names connector tools, not CLI commands, in its tool table", () => {
  const text = readFileSync(join(SKILL_DIR, "references", "pidash-mcp.md"), "utf8");
  const tools = new Set([...text.matchAll(/`(pidash_[a-z_]+)`/g)].map((m) => m[1]));

  assert.ok(tools.has("pidash_create_issue"));
  assert.ok(tools.size >= 25, `only found ${tools.size} connector tools`);
});
