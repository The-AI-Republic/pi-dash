// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI settings page (ADM-046, ADM-047, ADM-048, from
// parity/specs/admin.md §6.1–§6.3): the OpenAI model and API-key form
// over the shared configurations, the other-vendor callout, and the
// header/grid/button skeleton while configurations load. Both fields
// are optional and the save stays enabled; a failed configurations
// read offers a retry instead of hanging on the skeleton.
import { Button, IconButton, Input, Skeleton } from "@pidash/kit";
import { Eye, EyeOff } from "lucide-react";
import * as React from "react";

import {
  AI_KEY_DOCS_URL,
  AI_MODEL_DOCS_URL,
  AI_VENDOR_CONTACT_URL,
  LLM_API_KEY,
  LLM_MODEL_KEY,
  selectAiSettings,
  useSaveAiSettings,
  type AiSettingsValues,
} from "../aiSettings.js";
import { useInstanceConfigurations } from "../hooks.js";

/** Page title, matching the sidebar entry and crumb names. */
export const AI_SETTINGS_TITLE = "Artificial intelligence";

/** Loading skeleton: header, field grid, and save-button blocks. */
function AdminAiSkeleton(): React.ReactElement {
  return (
    <div
      data-testid="admin-ai-skeleton"
      role="status"
      aria-label="Loading AI settings"
      className="mx-auto flex w-full max-w-2xl flex-col gap-(--space-6) p-(--space-6)"
    >
      <Skeleton width="40%" height="28px" />
      <div className="grid grid-cols-1 gap-(--space-4)">
        <Skeleton height="68px" />
        <Skeleton height="68px" />
      </div>
      <Skeleton width="140px" height="36px" />
    </div>
  );
}

/** Failed configurations read: a retry, never a stuck skeleton. */
function AdminAiFailure({ onRetry }: { onRetry: () => void }): React.ReactElement {
  return (
    <div
      data-testid="admin-ai-error"
      role="alert"
      className="mx-auto flex w-full max-w-2xl flex-col items-start gap-(--space-4) p-(--space-6)"
    >
      <p className="text-body text-(--text)">The AI settings could not be loaded.</p>
      <Button type="button" variant="secondary" data-testid="admin-ai-retry" onClick={onRetry}>
        Try again
      </Button>
    </div>
  );
}

/** Static other-vendor callout: messaging only, nothing to configure. */
function AdminAiVendorCallout(): React.ReactElement {
  return (
    <section
      data-testid="admin-ai-callout"
      aria-labelledby="admin-ai-callout-heading"
      className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-4)"
    >
      <h2 id="admin-ai-callout-heading" className="text-emphasis text-(--text)">
        Prefer a different AI vendor?
      </h2>
      <p className="text-body text-(--text-muted)">
        If your organisation standardises on another provider,{" "}
        <a
          href={AI_VENDOR_CONTACT_URL}
          target="_blank"
          rel="noreferrer"
          data-testid="admin-ai-callout-link"
          className="font-medium text-(--accent) hover:text-(--accent-hover)"
        >
          get in touch
        </a>{" "}
        and tell us what you need.
      </p>
    </section>
  );
}

/** The model and key form. Mounted once configurations have loaded. */
function AdminAiSettingsForm({ initial }: { initial: AiSettingsValues }): React.ReactElement {
  const [model, setModel] = React.useState(initial.model);
  const [apiKey, setApiKey] = React.useState(initial.apiKey);
  const [keyVisible, setKeyVisible] = React.useState(false);
  const save = useSaveAiSettings();
  return (
    <form
      data-testid="admin-ai-form"
      className="flex flex-col gap-(--space-6)"
      onSubmit={(event) => {
        event.preventDefault();
        save.mutate({ model, apiKey });
      }}
    >
      <section aria-labelledby="admin-ai-openai-heading" className="flex flex-col gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <h2 id="admin-ai-openai-heading" className="text-h3 text-(--text)">
            OpenAI
          </h2>
          <p className="text-body text-(--text-muted)">
            The model and API key power AI features across every workspace on this instance. Both are optional — saving
            with empty fields turns those features off.
          </p>
        </div>
        <div className="flex flex-col gap-(--space-2)">
          <Input
            id={LLM_MODEL_KEY}
            label="Model"
            placeholder="gpt-4o"
            autoComplete="off"
            value={model}
            onChange={(event) => setModel(event.target.value)}
          />
          <p className="text-meta text-(--text-muted)">
            See the{" "}
            <a
              href={AI_MODEL_DOCS_URL}
              target="_blank"
              rel="noreferrer"
              data-testid="admin-ai-model-help"
              className="font-medium text-(--accent) hover:text-(--accent-hover)"
            >
              OpenAI model catalogue
            </a>{" "}
            for valid identifiers.
          </p>
        </div>
        <div className="flex flex-col gap-(--space-2)">
          <Input
            id={LLM_API_KEY}
            label="API key"
            placeholder="sk-..."
            autoComplete="off"
            type={keyVisible ? "text" : "password"}
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            trailing={
              <IconButton
                label={keyVisible ? "Hide API key" : "Show API key"}
                size="small"
                tabIndex={-1}
                aria-pressed={keyVisible}
                data-testid="admin-ai-key-toggle"
                onClick={() => setKeyVisible((visible) => !visible)}
              >
                {keyVisible ? <EyeOff size={16} /> : <Eye size={16} />}
              </IconButton>
            }
          />
          <p className="text-meta text-(--text-muted)">
            Create one on your{" "}
            <a
              href={AI_KEY_DOCS_URL}
              target="_blank"
              rel="noreferrer"
              data-testid="admin-ai-key-help"
              className="font-medium text-(--accent) hover:text-(--accent-hover)"
            >
              OpenAI API keys page
            </a>
            .
          </p>
        </div>
        <div>
          <Button type="submit" variant="primary" disabled={save.isPending} data-testid="admin-ai-save">
            {save.isPending ? "Saving…" : "Save changes"}
          </Button>
        </div>
      </section>
    </form>
  );
}

/**
 * The AI settings page: skeleton until the shared configurations
 * load, then the form plus the vendor callout.
 */
export function AdminAiSettingsPage(): React.ReactElement {
  const configurations = useInstanceConfigurations();
  if (configurations.data === undefined) {
    if (configurations.isError) {
      return <AdminAiFailure onRetry={() => void configurations.refetch()} />;
    }
    return <AdminAiSkeleton />;
  }
  return (
    <div data-testid="admin-ai-page" className="mx-auto flex w-full max-w-2xl flex-col gap-(--space-6) p-(--space-6)">
      <h1 className="text-h2 text-(--text)">{AI_SETTINGS_TITLE}</h1>
      <AdminAiSettingsForm initial={selectAiSettings(configurations.data)} />
      <AdminAiVendorCallout />
    </div>
  );
}
