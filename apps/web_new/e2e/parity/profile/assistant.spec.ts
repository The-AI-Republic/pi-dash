// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-428): the profile AI-assistant tab — BYO LLM
// provider (PROF-021), BYO STT dictation provider (PROF-022), and external
// MCP tool servers (PROF-023). Rows PROF-021, PROF-022, PROF-023.
//
// Observed behavior notes (the inventory rows carry them at update time):
// the tab stacks three sections (LLM form, Voice dictation, Tool servers)
// sharing Save/Test/Remove labels; the key fields clear on save and the
// API never echoes the key (has_api_key only); Test dials the provider
// and toasts its error code; a dead endpoint fails fast with
// provider_unreachable; real validation failures (non-http base URL)
// toast the generic Save-failed fallback while keeping the form, and the
// server message surfaces when the body carries one; the Voice section
// renders unflagged in this checkout (VOICE_DICTATION_ENABLED ships in
// main#586, still open — the flag-off half stays a Gap in PROF-022);
// tool-server adds reject duplicate names with the list kept.
import { test, expect } from "../fixtures";
import {
  browserSessionCookies,
  markOnboardedForProjects,
  parityApiBase,
  ROLE,
  seatFreshMember,
  serverAssistantConfigGet,
  serverAssistantMcpList,
  serverAssistantSttGet,
  serverRequestStatus,
  signInAuthedSession,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Dead endpoint: nothing listens, so probes fail fast and deterministically. */
const DEAD_URL = "http://127.0.0.1:9";

/** Seat a fresh workspace member, onboarded and ready to browse settings. */
async function freshSettingsUser(
  admin: AuthedSession,
  seed: ParitySeedFacts,
  prefix: string
): Promise<{ user: AuthedSession; cookies: ReturnType<typeof browserSessionCookies> }> {
  const user = await seatFreshMember(admin, seed.workspaceSlug, ROLE.MEMBER, prefix);
  await markOnboardedForProjects(user);
  return { user, cookies: browserSessionCookies(user) };
}

async function toastPoll(driver: ParityDriver): Promise<{ title: string; message: string } | null> {
  return driver.settingsProfileLastToast();
}

test(
  specTitle(["PROF-021"], "BYO LLM provider saves, tests, and removes without ever echoing the key"),
  { tag: specTags(["PROF-021"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const { user, cookies } = await freshSettingsUser(admin, seed, "parity-prof21");
    const api = parityApiBase();

    await test.step("config starts empty server-side", async () => {
      const config = await serverAssistantConfigGet(user.cookie);
      expect(config.has_api_key).toBe(false);
      expect(config.model_name).toBe("");
    });

    await test.step("keyless form disables save/test and hides remove", async () => {
      await driver.settingsProfileOpenTab("ai-assistant", cookies);
      const facts = await driver.settingsProfileAssistantFacts();
      expect(facts.provider).toBe("openai_compatible");
      expect(facts.baseUrlVisible).toBe(true);
      expect(facts.baseUrl).toBe("");
      expect(facts.model).toBe("");
      expect(facts.keyValue).toBe("");
      expect(facts.keyPlaceholder).toBe("Your provider API key");
      expect(facts.saveDisabled).toBe(true);
      expect(facts.testDisabled).toBe(true);
      expect(facts.removeVisible).toBe(false);
      expect(facts.lastVerified).toBeNull();
    });

    await test.step("real validation failure keeps the form and toasts", async () => {
      await driver.settingsProfileAssistantFill("baseUrl", "notaurl");
      await driver.settingsProfileAssistantFill("model", "probe-model");
      await driver.settingsProfileAssistantFill("key", "parity-dummy-key-21");
      expect((await driver.settingsProfileAssistantFacts()).saveDisabled).toBe(false);
      await driver.settingsProfileAssistantSave();
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Save failed",
          message: "Invalid configuration",
        });
      const facts = await driver.settingsProfileAssistantFacts();
      expect(facts.baseUrl).toBe("notaurl");
      expect(facts.model).toBe("probe-model");
      expect((await serverAssistantConfigGet(user.cookie)).has_api_key).toBe(false);
    });

    await test.step("server-message failure surfaces the message and keeps the form", async () => {
      await driver.settingsProfileAssistantFill("baseUrl", DEAD_URL);
      await driver.settingsProfileAssistantFailWrites();
      try {
        await driver.settingsProfileAssistantSave();
        await expect
          .poll(() => toastPoll(driver), { timeout: 15_000 })
          .toEqual({
            title: "Save failed",
            message: "probe_boom",
          });
      } finally {
        await driver.settingsProfileAssistantClearRoutes();
      }
      const facts = await driver.settingsProfileAssistantFacts();
      expect(facts.baseUrl).toBe(DEAD_URL);
      expect((await serverAssistantConfigGet(user.cookie)).has_api_key).toBe(false);
    });

    await test.step("save stores the config and clears the key field", async () => {
      await driver.settingsProfileAssistantSave();
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Saved",
          message: "AI provider configuration updated.",
        });
      const config = await serverAssistantConfigGet(user.cookie);
      expect(config.provider_kind).toBe("openai_compatible");
      expect(config.base_url).toBe(DEAD_URL);
      expect(config.model_name).toBe("probe-model");
      expect(config.has_api_key).toBe(true);
      const facts = await driver.settingsProfileAssistantFacts();
      expect(facts.keyValue).toBe("");
      expect(facts.keyPlaceholder).toBe("•••• (saved) — enter to replace");
      expect(facts.testDisabled).toBe(false);
      expect(facts.removeVisible).toBe(true);
    });

    await test.step("the key is never echoed back", async () => {
      const raw = await serverRequestStatus("GET", `${api}/api/users/me/ai-assistant/config/`, user.cookie);
      expect(raw.status).toBe(200);
      expect(raw.bodyText).not.toContain("parity-dummy-key-21");
      expect(raw.bodyText).not.toContain('"api_key"');
    });

    await test.step("testing a dead endpoint toasts its failure code", async () => {
      await driver.settingsProfileAssistantTest();
      await expect
        .poll(() => toastPoll(driver), { timeout: 30_000 })
        .toEqual({
          title: "Connection failed",
          message: "provider_unreachable",
        });
      expect((await serverAssistantConfigGet(user.cookie)).last_verified_at).toBeNull();
      expect((await driver.settingsProfileAssistantFacts()).lastVerified).toBeNull();
    });

    await test.step("reopening shows the saved values with the key still blank", async () => {
      await driver.settingsProfileOpenTab("ai-assistant", cookies);
      // Values fill from the data read after paint; poll until they land.
      await expect
        .poll(async () => (await driver.settingsProfileAssistantFacts()).model, { timeout: 15_000 })
        .toBe("probe-model");
      const facts = await driver.settingsProfileAssistantFacts();
      expect(facts.baseUrl).toBe(DEAD_URL);
      expect(facts.keyValue).toBe("");
    });

    await test.step("anthropic hides the base URL and saves its kind", async () => {
      await driver.settingsProfileAssistantSetProvider("anthropic");
      expect((await driver.settingsProfileAssistantFacts()).baseUrlVisible).toBe(false);
      await driver.settingsProfileAssistantSave();
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Saved",
          message: "AI provider configuration updated.",
        });
      expect((await serverAssistantConfigGet(user.cookie)).provider_kind).toBe("anthropic");
    });

    await test.step("remove clears the config and restores the keyless form", async () => {
      await driver.settingsProfileAssistantRemove();
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Removed",
          message: "AI provider configuration deleted.",
        });
      const config = await serverAssistantConfigGet(user.cookie);
      expect(config.has_api_key).toBe(false);
      expect(config.model_name).toBe("");
      const facts = await driver.settingsProfileAssistantFacts();
      expect(facts.removeVisible).toBe(false);
      expect(facts.testDisabled).toBe(true);
      expect(facts.keyPlaceholder).toBe("Your provider API key");
    });

    await test.step("blank writes refuse server-side", async () => {
      const raw = await serverRequestStatus("PUT", `${api}/api/users/me/ai-assistant/config/`, user.cookie, {});
      expect(raw.status).toBe(400);
    });
  }
);

test(
  specTitle(["PROF-022"], "BYO dictation provider mirrors the assistant key lifecycle unflagged"),
  { tag: specTags(["PROF-022"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const { user, cookies } = await freshSettingsUser(admin, seed, "parity-prof22");
    const api = parityApiBase();

    await test.step("dictation config starts empty server-side", async () => {
      const config = await serverAssistantSttGet(user.cookie);
      expect(config.has_api_key).toBe(false);
      expect(config.model_name).toBe("");
    });

    await test.step("the voice section renders with save gated on both fields", async () => {
      await driver.settingsProfileOpenTab("ai-assistant", cookies);
      const facts = await driver.settingsProfileDictationFacts();
      expect(facts.sectionVisible).toBe(true);
      expect(facts.baseUrl).toBe("");
      expect(facts.model).toBe("");
      expect(facts.keyValue).toBe("");
      expect(facts.keyPlaceholder).toBe("Your transcription API key");
      expect(facts.saveDisabled).toBe(true);
      expect(facts.testDisabled).toBe(true);
      expect(facts.removeVisible).toBe(false);
      expect(facts.lastVerified).toBeNull();
      await driver.settingsProfileDictationFill("baseUrl", DEAD_URL);
      expect((await driver.settingsProfileDictationFacts()).saveDisabled).toBe(true);
    });

    await test.step("real validation failure keeps the form and toasts", async () => {
      await driver.settingsProfileDictationFill("baseUrl", "notaurl");
      await driver.settingsProfileDictationFill("model", "whisper-1");
      await driver.settingsProfileDictationFill("key", "parity-dummy-stt-22");
      await driver.settingsProfileDictationSave();
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Save failed",
          message: "Invalid configuration",
        });
      expect((await driver.settingsProfileDictationFacts()).baseUrl).toBe("notaurl");
      expect((await serverAssistantSttGet(user.cookie)).has_api_key).toBe(false);
    });

    await test.step("server-message failure surfaces the message and keeps the form", async () => {
      await driver.settingsProfileDictationFill("baseUrl", DEAD_URL);
      await driver.settingsProfileAssistantFailWrites();
      try {
        await driver.settingsProfileDictationSave();
        await expect
          .poll(() => toastPoll(driver), { timeout: 15_000 })
          .toEqual({
            title: "Save failed",
            message: "probe_boom",
          });
      } finally {
        await driver.settingsProfileAssistantClearRoutes();
      }
      expect((await driver.settingsProfileDictationFacts()).baseUrl).toBe(DEAD_URL);
      expect((await serverAssistantSttGet(user.cookie)).has_api_key).toBe(false);
    });

    await test.step("save stores the config and clears the key field", async () => {
      await driver.settingsProfileDictationSave();
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Saved",
          message: "Voice dictation configuration updated.",
        });
      const config = await serverAssistantSttGet(user.cookie);
      expect(config.base_url).toBe(DEAD_URL);
      expect(config.model_name).toBe("whisper-1");
      expect(config.has_api_key).toBe(true);
      const facts = await driver.settingsProfileDictationFacts();
      expect(facts.keyValue).toBe("");
      expect(facts.keyPlaceholder).toBe("•••• (saved) — enter to replace");
      expect(facts.testDisabled).toBe(false);
      expect(facts.removeVisible).toBe(true);
    });

    await test.step("the key is never echoed back", async () => {
      const raw = await serverRequestStatus("GET", `${api}/api/users/me/ai-assistant/stt-config/`, user.cookie);
      expect(raw.status).toBe(200);
      expect(raw.bodyText).not.toContain("parity-dummy-stt-22");
      expect(raw.bodyText).not.toContain('"api_key"');
    });

    await test.step("testing a dead endpoint toasts its failure code", async () => {
      await driver.settingsProfileDictationTest();
      await expect
        .poll(() => toastPoll(driver), { timeout: 30_000 })
        .toEqual({
          title: "Connection failed",
          message: "provider_unreachable",
        });
      expect((await serverAssistantSttGet(user.cookie)).last_verified_at).toBeNull();
    });

    await test.step("remove clears the config and restores the keyless form", async () => {
      await driver.settingsProfileDictationRemove();
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Removed",
          message: "Voice dictation configuration deleted.",
        });
      const config = await serverAssistantSttGet(user.cookie);
      expect(config.has_api_key).toBe(false);
      const facts = await driver.settingsProfileDictationFacts();
      expect(facts.removeVisible).toBe(false);
      expect(facts.testDisabled).toBe(true);
    });
  }
);

test(
  specTitle(["PROF-023"], "tool servers add, disable, and remove; failures keep the list"),
  { tag: specTags(["PROF-023"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const { user, cookies } = await freshSettingsUser(admin, seed, "parity-prof23");

    await test.step("registry starts empty with add gated on name and URL", async () => {
      expect(await serverAssistantMcpList(user.cookie)).toEqual([]);
      await driver.settingsProfileOpenTab("ai-assistant", cookies);
      expect(await driver.settingsProfileToolServerRows()).toEqual([]);
      expect(await driver.settingsProfileToolServerAddDisabled()).toBe(true);
    });

    await test.step("add without credentials lists unlocked with its prefix", async () => {
      await driver.settingsProfileToolServerAdd("probe-mcp-a", "https://mcp-a.example.com/sse");
      await expect
        .poll(async () => (await driver.settingsProfileToolServerRows()).map((row) => row.name), { timeout: 15_000 })
        .toEqual(["probe-mcp-a"]);
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Added",
          message: "Tool server added.",
        });
      const rows = await driver.settingsProfileToolServerRows();
      expect(rows).toHaveLength(1);
      expect(rows[0]).toMatchObject({ name: "probe-mcp-a", url: "https://mcp-a.example.com/sse", disabled: false });
      expect(rows[0]?.locked).toBe(false);
      expect(rows[0]?.prefix ?? "").toMatch(/_\*$/);
      const server = await serverAssistantMcpList(user.cookie);
      expect(server).toHaveLength(1);
      expect(server[0]).toMatchObject({ name: "probe-mcp-a", has_auth_header: false, is_enabled: true });
    });

    await test.step("add with an auth header lists locked", async () => {
      await driver.settingsProfileToolServerAdd("probe-mcp-b", "https://mcp-b.example.com/mcp", "Bearer parity-x");
      await expect
        .poll(async () => (await driver.settingsProfileToolServerRows()).map((row) => row.name), { timeout: 15_000 })
        .toEqual(["probe-mcp-a", "probe-mcp-b"]);
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Added",
          message: "Tool server added.",
        });
      const rows = await driver.settingsProfileToolServerRows();
      expect(rows[1]).toMatchObject({ disabled: false, locked: true });
      const server = await serverAssistantMcpList(user.cookie);
      expect(server.find((row) => row.name === "probe-mcp-b")?.has_auth_header).toBe(true);
    });

    await test.step("duplicate names fail with the list kept", async () => {
      await driver.settingsProfileToolServerAdd("probe-mcp-a", "https://mcp-a.example.com/other");
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Could not add server",
          message: "You already have a tool server with that name.",
        });
      const rows = await driver.settingsProfileToolServerRows();
      expect(rows.map((row) => row.name)).toEqual(["probe-mcp-a", "probe-mcp-b"]);
      expect((await serverAssistantMcpList(user.cookie)).map((row) => row.name)).toEqual([
        "probe-mcp-a",
        "probe-mcp-b",
      ]);
    });

    await test.step("disable marks the row and drops its prefix; re-enable restores both", async () => {
      await driver.settingsProfileToolServerToggle("probe-mcp-a");
      await expect
        .poll(async () => (await driver.settingsProfileToolServerRows())[0], { timeout: 15_000 })
        .toMatchObject({ name: "probe-mcp-a", disabled: true, prefix: null });
      expect((await serverAssistantMcpList(user.cookie)).find((row) => row.name === "probe-mcp-a")?.is_enabled).toBe(
        false
      );
      await driver.settingsProfileToolServerToggle("probe-mcp-a");
      await expect
        .poll(async () => (await driver.settingsProfileToolServerRows())[0], { timeout: 15_000 })
        .toMatchObject({ name: "probe-mcp-a", disabled: false });
      const back = await driver.settingsProfileToolServerRows();
      expect(back[0]).toMatchObject({ name: "probe-mcp-a", disabled: false });
      expect(back[0]?.prefix ?? "").toMatch(/_\*$/);
    });

    await test.step("remove drops each row until the registry is empty", async () => {
      await driver.settingsProfileToolServerRemove("probe-mcp-b");
      await expect
        .poll(async () => (await driver.settingsProfileToolServerRows()).map((row) => row.name), { timeout: 15_000 })
        .toEqual(["probe-mcp-a"]);
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Removed",
          message: "probe-mcp-b removed.",
        });
      await driver.settingsProfileToolServerRemove("probe-mcp-a");
      await expect
        .poll(async () => (await driver.settingsProfileToolServerRows()).map((row) => row.name), { timeout: 15_000 })
        .toEqual([]);
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "Removed",
          message: "probe-mcp-a removed.",
        });
      expect(await serverAssistantMcpList(user.cookie)).toEqual([]);
    });
  }
);
