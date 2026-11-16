// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider credential form shell (ADM-032–035, from parity/specs/admin.md
// §4.10–4.13): the credential inputs over a draft, the staged sync
// switch, the copy-out URL fields, and the save/back row with the
// discard guard. Save stays disabled until the form dirties, shows a
// busy label in flight, and on success re-reads its defaults from the
// server echo; required gaps block silently, and a failed save only
// logs (the accepted OSS quirk). Credential field sets arrive in
// NEWFRONT-511; the sync/copy/save/guard behavior here is final.
import { Button, IconButton, Input, Switch } from "@pidash/kit";
import { Eye, EyeOff } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifySuccess } from "../../../shared/shell/index.js";
import type { ConfigMap } from "../authMethods.js";
import { useUpdateInstanceConfigurations } from "../hooks.js";
import {
  AUTH_PROVIDER_LANDING_HREF,
  buildProviderCopyFields,
  buildProviderFormDefaults,
  buildProviderSavePayload,
  decodeSyncFlag,
  draftValue,
  isProviderFormDirty,
  isProviderFormSubmittable,
  PROVIDER_BACK_LABEL,
  PROVIDER_SAVE_BUSY_LABEL,
  PROVIDER_SAVE_DONE_TITLE,
  PROVIDER_SAVE_IDLE_LABEL,
  providerSaveDoneMessage,
  readProviderSavedValues,
  resolveAuthOrigin,
  type AuthProviderDef,
  type CopyHelpHref,
  type ProviderCredentialField,
} from "../providerAuth.js";
import { AdminAuthCopyField } from "./AdminAuthCopyField.js";
import { AdminAuthDiscardModal } from "./AdminAuthDiscardModal.js";

export interface ProviderCopyHelp {
  href: CopyHelpHref;
  label: string;
}

export interface AdminAuthProviderFormProps {
  provider: AuthProviderDef;
  config: ConfigMap;
  /** Credential inputs to render (field sets arrive in NEWFRONT-511). */
  fields?: readonly ProviderCredentialField[] | undefined;
  /** Stored-value prefill per key (the Gitea host default arrives in NEWFRONT-511). */
  prefill?: ((key: string, stored: string) => string) | undefined;
  /** Copy-field help copy keyed by field label (provider-console guidance arrives in NEWFRONT-511). */
  copyDescriptions?: Partial<Record<string, React.ReactNode>> | undefined;
  /** Copy-field help links keyed by field label. */
  copyHelp?: Partial<Record<string, ProviderCopyHelp>> | undefined;
}

export function AdminAuthProviderForm({
  provider,
  config,
  fields = [],
  prefill,
  copyDescriptions,
  copyHelp,
}: AdminAuthProviderFormProps): React.ReactElement {
  const save = useUpdateInstanceConfigurations();
  const [defaults, setDefaults] = React.useState(() =>
    buildProviderFormDefaults(config, fields, provider.syncKey, prefill)
  );
  const [values, setValues] = React.useState(defaults.fields);
  const [syncOn, setSyncOn] = React.useState(defaults.syncOn);
  const [discardOpen, setDiscardOpen] = React.useState(false);
  const [revealed, setRevealed] = React.useState<Record<string, boolean>>({});
  const origin = resolveAuthOrigin(getAppClient().baseUrl);
  const copyFields = buildProviderCopyFields(provider, origin);
  const dirty = isProviderFormDirty({ fields: values, syncOn }, defaults);
  const syncLabelId = `admin-auth-provider-sync-label-${provider.slug}`;

  const setField = (key: string, next: string): void => {
    setValues((current) => ({ ...current, [key]: next }));
  };

  const toggleRevealed = (key: string): void => {
    setRevealed((current) => ({ ...current, [key]: current[key] !== true }));
  };

  const onSubmit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (save.isPending) return;
    if (!isProviderFormSubmittable(values, fields)) return;
    const payload = buildProviderSavePayload(values, syncOn, provider.syncKey);
    save.mutate(payload, {
      // No onError: a failed provider save only logs through the shared
      // hook (the accepted log-only quirk), with no notice.
      onSuccess: (rows) => {
        const saved = readProviderSavedValues(rows, payload);
        const { [provider.syncKey]: savedSyncFlag, ...savedFields } = saved;
        const nextSync = decodeSyncFlag(savedSyncFlag);
        setDefaults({ fields: savedFields, syncOn: nextSync });
        setValues(savedFields);
        setSyncOn(nextSync);
        notifySuccess(PROVIDER_SAVE_DONE_TITLE, providerSaveDoneMessage(provider.name));
      },
    });
  };

  const handleBack = (event: React.MouseEvent<HTMLAnchorElement>): void => {
    event.preventDefault();
    setDiscardOpen(true);
  };

  return (
    <>
      <AdminAuthDiscardModal
        open={discardOpen}
        backHref={AUTH_PROVIDER_LANDING_HREF}
        onKeep={() => setDiscardOpen(false)}
      />
      <form
        data-testid="admin-auth-provider-form"
        onSubmit={onSubmit}
        aria-busy={save.isPending}
        className="flex w-full flex-col gap-(--space-6)"
      >
        {fields.map((field) => {
          const visible = revealed[field.key] === true;
          return (
            <div key={field.key} className="flex flex-col gap-(--space-1)">
              <Input
                id={field.key}
                label={field.label}
                type={field.type === "password" && !visible ? "password" : "text"}
                value={draftValue(values, field.key)}
                onChange={(event) => setField(field.key, event.target.value)}
                data-testid={`admin-auth-field-${field.key}`}
                {...(field.placeholder === undefined ? {} : { placeholder: field.placeholder })}
                {...(field.type === "password"
                  ? {
                      trailing: (
                        <IconButton
                          label={visible ? `Hide ${field.label}` : `Show ${field.label}`}
                          size="small"
                          onClick={() => toggleRevealed(field.key)}
                        >
                          {visible ? <EyeOff size={16} /> : <Eye size={16} />}
                        </IconButton>
                      ),
                    }
                  : {})}
              />
              {field.description === undefined ? null : (
                <div className="text-meta text-(--text-muted)">{field.description}</div>
              )}
            </div>
          );
        })}
        <div className="flex w-full items-center justify-between gap-(--space-4)">
          <p id={syncLabelId} className="text-body font-medium text-(--text)">
            {provider.syncLabel}
          </p>
          <Switch
            checked={syncOn}
            onCheckedChange={setSyncOn}
            aria-labelledby={syncLabelId}
            data-testid="admin-auth-provider-sync"
          />
        </div>
        <div className="flex flex-col gap-(--space-4)">
          {copyFields.map((copy) => {
            const help = copyHelp?.[copy.label];
            return (
              <AdminAuthCopyField
                key={copy.label}
                label={copy.label}
                url={copy.url}
                description={copyDescriptions?.[copy.label]}
                helpHref={help?.href}
                helpLabel={help?.label}
                fieldValues={values}
              />
            );
          })}
        </div>
        <div className="flex items-center gap-(--space-4)">
          <Button
            type="submit"
            variant="primary"
            loading={save.isPending}
            disabled={!dirty}
            data-testid="admin-auth-provider-save"
          >
            {save.isPending ? PROVIDER_SAVE_BUSY_LABEL : PROVIDER_SAVE_IDLE_LABEL}
          </Button>
          <a
            href={AUTH_PROVIDER_LANDING_HREF}
            onClick={dirty ? handleBack : undefined}
            data-testid="admin-auth-provider-back"
            className="text-body font-medium text-(--accent) hover:underline"
          >
            {PROVIDER_BACK_LABEL}
          </a>
        </div>
      </form>
    </>
  );
}
