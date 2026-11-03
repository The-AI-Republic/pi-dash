// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider registry and shared form helpers: pinned identity strings,
// origin/callback derivation, sync codec, defaults/dirty/gating/payload
// helpers, and the copy-field assembly per provider.
import type { InstanceConfigurationList } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ConfigMap } from "./authMethods.js";
import {
  AUTH_PROVIDER_BY_SLUG,
  AUTH_PROVIDER_LANDING_HREF,
  AUTH_PROVIDER_SKELETON_BARS,
  AUTH_PROVIDERS,
  buildAuthCallbackUrl,
  buildProviderCopyFields,
  buildProviderFormDefaults,
  buildProviderSavePayload,
  decodeSyncFlag,
  draftValue,
  encodeSyncFlag,
  isProviderFormDirty,
  isProviderFormSubmittable,
  PROVIDER_BACK_LABEL,
  PROVIDER_COPY_DONE_TITLE,
  PROVIDER_DISCARD_BODY,
  PROVIDER_DISCARD_KEEP_LABEL,
  PROVIDER_DISCARD_TITLE,
  PROVIDER_MASTER_FAILED_MESSAGE,
  PROVIDER_MASTER_FAILED_TITLE,
  PROVIDER_MASTER_SAVED_TITLE,
  PROVIDER_SAVE_BUSY_LABEL,
  PROVIDER_SAVE_DONE_TITLE,
  PROVIDER_SAVE_IDLE_LABEL,
  providerCopyDoneMessage,
  providerMasterSavedMessage,
  providerSaveDoneMessage,
  readProviderSavedValues,
  resolveAuthOrigin,
  resolveCopyHelpHref,
} from "./providerAuth.js";
import type { ProviderCredentialField } from "./providerAuth.js";

const FIELDS: readonly ProviderCredentialField[] = [
  { key: "CLIENT_ID", label: "Client ID", type: "text", required: true },
  { key: "CLIENT_SECRET", label: "Client secret", type: "password", required: true },
  { key: "ORG_ID", label: "Organization ID", type: "text", required: false },
];

type ConfigRow = InstanceConfigurationList[number];

function configRow(key: string, value: string | null): ConfigRow {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "AUTHENTICATION",
    is_encrypted: false,
    source: "db",
    is_managed: false,
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("provider registry", () => {
  it("lists the four providers with their pinned identity strings", () => {
    expect(AUTH_PROVIDERS.map((provider) => provider.slug)).toEqual(["github", "gitlab", "gitea", "google"]);
    expect(AUTH_PROVIDERS.map((provider) => provider.name)).toEqual(["GitHub", "GitLab", "Gitea", "Google"]);
    for (const provider of AUTH_PROVIDERS) {
      expect(provider.description).toBe(
        `Allow members to login or sign up to pi dash with their ${provider.name} accounts.`
      );
      expect(provider.title).toBe(`${provider.name} Authentication - God Mode`);
      expect(provider.syncLabel).toBe(`Refresh user attributes from ${provider.name} during sign in`);
    }
  });

  it("carries the enable and sync keys per provider", () => {
    expect(AUTH_PROVIDER_BY_SLUG.github.enableKey).toBe("IS_GITHUB_ENABLED");
    expect(AUTH_PROVIDER_BY_SLUG.github.syncKey).toBe("ENABLE_GITHUB_SYNC");
    expect(AUTH_PROVIDER_BY_SLUG.gitlab.enableKey).toBe("IS_GITLAB_ENABLED");
    expect(AUTH_PROVIDER_BY_SLUG.gitlab.syncKey).toBe("ENABLE_GITLAB_SYNC");
    expect(AUTH_PROVIDER_BY_SLUG.gitea.enableKey).toBe("IS_GITEA_ENABLED");
    expect(AUTH_PROVIDER_BY_SLUG.gitea.syncKey).toBe("ENABLE_GITEA_SYNC");
    expect(AUTH_PROVIDER_BY_SLUG.google.enableKey).toBe("IS_GOOGLE_ENABLED");
    expect(AUTH_PROVIDER_BY_SLUG.google.syncKey).toBe("ENABLE_GOOGLE_SYNC");
  });

  it("marks which providers show the origin field and the callback label spelling", () => {
    expect(AUTH_PROVIDER_BY_SLUG.github.showsOriginCopy).toBe(true);
    expect(AUTH_PROVIDER_BY_SLUG.google.showsOriginCopy).toBe(true);
    expect(AUTH_PROVIDER_BY_SLUG.gitlab.showsOriginCopy).toBe(false);
    expect(AUTH_PROVIDER_BY_SLUG.gitea.showsOriginCopy).toBe(false);
    expect(AUTH_PROVIDER_BY_SLUG.github.callbackLabel).toBe("Callback URI");
    expect(AUTH_PROVIDER_BY_SLUG.google.callbackLabel).toBe("Callback URI");
    expect(AUTH_PROVIDER_BY_SLUG.gitea.callbackLabel).toBe("Callback URI");
    expect(AUTH_PROVIDER_BY_SLUG.gitlab.callbackLabel).toBe("Callback URL");
  });

  it("pins the skeleton bar count and the landing href", () => {
    expect(AUTH_PROVIDER_SKELETON_BARS).toBe(5);
    expect(AUTH_PROVIDER_LANDING_HREF).toBe("/god-mode/authentication/");
  });
});

describe("notice and label wording", () => {
  it("builds the master-toggle success message per direction", () => {
    expect(PROVIDER_MASTER_SAVED_TITLE).toBe("Configuration saved");
    expect(providerMasterSavedMessage("GitHub", true)).toBe("GitHub authentication is now active.");
    expect(providerMasterSavedMessage("GitHub", false)).toBe("GitHub authentication is now disabled.");
  });

  it("pins the master-toggle failure wording", () => {
    expect(PROVIDER_MASTER_FAILED_TITLE).toBe("Error");
    expect(PROVIDER_MASTER_FAILED_MESSAGE).toBe("Failed to save configuration");
  });

  it("builds the credential-save Done message per provider", () => {
    expect(PROVIDER_SAVE_DONE_TITLE).toBe("Done!");
    expect(providerSaveDoneMessage("GitLab")).toBe("Your GitLab authentication is configured. You should test it now.");
  });

  it("pins the save labels", () => {
    expect(PROVIDER_SAVE_IDLE_LABEL).toBe("Save changes");
    expect(PROVIDER_SAVE_BUSY_LABEL).toBe("Saving");
  });

  it("pins the discard modal and back wording", () => {
    expect(PROVIDER_DISCARD_TITLE).toBe("You have unsaved changes");
    expect(PROVIDER_DISCARD_BODY).toBe("Changes you made will be lost if you go back. Do you wish to go back?");
    expect(PROVIDER_DISCARD_KEEP_LABEL).toBe("Keep editing");
    expect(PROVIDER_BACK_LABEL).toBe("Go back");
  });

  it("builds the copy notice per label", () => {
    expect(PROVIDER_COPY_DONE_TITLE).toBe("Copied to clipboard");
    expect(providerCopyDoneMessage("Callback URI")).toBe(
      "The Callback URI has been successfully copied to your clipboard"
    );
  });
});

describe("sync flag codec", () => {
  it("decodes only the 1 string as on", () => {
    expect(decodeSyncFlag("1")).toBe(true);
    expect(decodeSyncFlag("0")).toBe(false);
    expect(decodeSyncFlag("")).toBe(false);
    expect(decodeSyncFlag(null)).toBe(false);
    expect(decodeSyncFlag(undefined)).toBe(false);
  });

  it("encodes the switch as the string flag", () => {
    expect(encodeSyncFlag(true)).toBe("1");
    expect(encodeSyncFlag(false)).toBe("0");
  });

  it("reads draft values with blank for missing keys", () => {
    expect(draftValue({ CLIENT_ID: "id-1" }, "CLIENT_ID")).toBe("id-1");
    expect(draftValue({}, "CLIENT_ID")).toBe("");
  });
});

describe("form defaults", () => {
  it("reads stored values with the sync flag on", () => {
    const config: ConfigMap = {
      CLIENT_ID: "id-1",
      CLIENT_SECRET: "secret-1",
      ORG_ID: "org-1",
      SYNC_KEY: "1",
    };
    expect(buildProviderFormDefaults(config, FIELDS, "SYNC_KEY")).toEqual({
      fields: { CLIENT_ID: "id-1", CLIENT_SECRET: "secret-1", ORG_ID: "org-1" },
      syncOn: true,
    });
  });

  it("blanks unset credentials and reads the sync switch off", () => {
    const config: ConfigMap = { CLIENT_ID: null };
    expect(buildProviderFormDefaults(config, FIELDS, "SYNC_KEY")).toEqual({
      fields: { CLIENT_ID: "", CLIENT_SECRET: "", ORG_ID: "" },
      syncOn: false,
    });
  });

  it("runs stored values through the prefill", () => {
    const config: ConfigMap = { CLIENT_ID: "" };
    const defaults = buildProviderFormDefaults(config, FIELDS, "SYNC_KEY", (key, stored) =>
      key === "CLIENT_ID" && stored === "" ? "prefilled" : stored
    );
    expect(defaults.fields["CLIENT_ID"]).toBe("prefilled");
    expect(defaults.fields["ORG_ID"]).toBe("");
  });
});

describe("dirty check", () => {
  it("reads clean when nothing changed", () => {
    const values = { fields: { CLIENT_ID: "id-1" }, syncOn: false };
    expect(isProviderFormDirty(values, { fields: { CLIENT_ID: "id-1" }, syncOn: false })).toBe(false);
  });

  it("reads dirty on a sync flip alone", () => {
    expect(
      isProviderFormDirty(
        { fields: { CLIENT_ID: "id-1" }, syncOn: true },
        { fields: { CLIENT_ID: "id-1" }, syncOn: false }
      )
    ).toBe(true);
  });

  it("reads dirty on a credential change", () => {
    expect(
      isProviderFormDirty(
        { fields: { CLIENT_ID: "id-2" }, syncOn: false },
        { fields: { CLIENT_ID: "id-1" }, syncOn: false }
      )
    ).toBe(true);
  });

  it("treats keys present on one side as changed", () => {
    expect(isProviderFormDirty({ fields: { EXTRA: "x" }, syncOn: false }, { fields: {}, syncOn: false })).toBe(true);
    expect(isProviderFormDirty({ fields: {}, syncOn: false }, { fields: { EXTRA: "x" }, syncOn: false })).toBe(true);
  });
});

describe("submit gating", () => {
  it("passes when every required credential holds a value", () => {
    expect(isProviderFormSubmittable({ CLIENT_ID: "id", CLIENT_SECRET: "secret" }, FIELDS)).toBe(true);
  });

  it("blocks when a required credential is blank or missing", () => {
    expect(isProviderFormSubmittable({ CLIENT_ID: "", CLIENT_SECRET: "secret" }, FIELDS)).toBe(false);
    expect(isProviderFormSubmittable({ CLIENT_SECRET: "secret" }, FIELDS)).toBe(false);
  });

  it("ignores blank optional credentials", () => {
    expect(isProviderFormSubmittable({ CLIENT_ID: "id", CLIENT_SECRET: "secret", ORG_ID: "" }, FIELDS)).toBe(true);
  });
});

describe("save payload and echo re-read", () => {
  it("carries every credential plus the staged sync flag", () => {
    expect(buildProviderSavePayload({ CLIENT_ID: "id" }, true, "SYNC_KEY")).toEqual({
      CLIENT_ID: "id",
      SYNC_KEY: "1",
    });
    expect(buildProviderSavePayload({ CLIENT_ID: "id" }, false, "SYNC_KEY")).toEqual({
      CLIENT_ID: "id",
      SYNC_KEY: "0",
    });
  });

  it("re-reads each submitted key from the echo", () => {
    const response = [configRow("CLIENT_ID", "echo-id"), configRow("SYNC_KEY", "1")];
    expect(readProviderSavedValues(response, { CLIENT_ID: "id", SYNC_KEY: "0" })).toEqual({
      CLIENT_ID: "echo-id",
      SYNC_KEY: "1",
    });
  });

  it("keeps the submitted value when the echo omits a key", () => {
    expect(readProviderSavedValues([configRow("CLIENT_ID", "echo-id")], { CLIENT_ID: "id", ORG_ID: "" })).toEqual({
      CLIENT_ID: "echo-id",
      ORG_ID: "",
    });
  });
});

describe("origin and callback derivation", () => {
  it("uses the configured API base", () => {
    expect(resolveAuthOrigin("https://api.example.test")).toBe("https://api.example.test");
  });

  it("trims trailing slashes from the base", () => {
    expect(resolveAuthOrigin("https://api.example.test///")).toBe("https://api.example.test");
  });

  it("falls back to the window origin when unconfigured", () => {
    vi.stubGlobal("window", { location: { origin: "https://window.example.test" } });
    expect(resolveAuthOrigin("")).toBe("https://window.example.test");
    expect(resolveAuthOrigin("///")).toBe("https://window.example.test");
  });

  it("reads blank with neither a base nor a window", () => {
    expect(resolveAuthOrigin("")).toBe("");
  });

  it("derives callbacks from the origin plus the provider path", () => {
    expect(buildAuthCallbackUrl("https://app.example.test", "github")).toBe(
      "https://app.example.test/auth/github/callback/"
    );
    expect(buildAuthCallbackUrl("https://app.example.test/", "gitea")).toBe(
      "https://app.example.test/auth/gitea/callback/"
    );
  });
});

describe("copy field assembly", () => {
  it("shows origin plus callback where the provider carries both", () => {
    expect(buildProviderCopyFields(AUTH_PROVIDER_BY_SLUG.github, "https://app.example.test")).toEqual([
      { label: "Origin URL", url: "https://app.example.test" },
      { label: "Callback URI", url: "https://app.example.test/auth/github/callback/" },
    ]);
  });

  it("shows the callback alone on GitLab and Gitea", () => {
    expect(buildProviderCopyFields(AUTH_PROVIDER_BY_SLUG.gitlab, "https://app.example.test")).toEqual([
      { label: "Callback URL", url: "https://app.example.test/auth/gitlab/callback/" },
    ]);
    expect(buildProviderCopyFields(AUTH_PROVIDER_BY_SLUG.gitea, "https://app.example.test")).toEqual([
      { label: "Callback URI", url: "https://app.example.test/auth/gitea/callback/" },
    ]);
  });

  it("normalizes the base so the callback matches the shown origin", () => {
    const fields = buildProviderCopyFields(AUTH_PROVIDER_BY_SLUG.google, "https://app.example.test//");
    expect(fields[0]?.url).toBe("https://app.example.test");
    expect(fields[1]?.url).toBe("https://app.example.test/auth/google/callback/");
  });
});

describe("copy help target", () => {
  it("reads null without a link", () => {
    expect(resolveCopyHelpHref(undefined, {})).toBeNull();
  });

  it("passes a static link through", () => {
    expect(resolveCopyHelpHref("https://console.example.test/new", {})).toBe("https://console.example.test/new");
  });

  it("derives the link from the live draft", () => {
    const href = resolveCopyHelpHref((values) => `${values["HOST"]}/settings`, { HOST: "https://h.example.test" });
    expect(href).toBe("https://h.example.test/settings");
  });
});
