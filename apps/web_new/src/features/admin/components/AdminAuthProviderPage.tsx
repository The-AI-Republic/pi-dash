// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider sub-page container (ADM-027, from parity/specs/admin.md §4.5):
// the shell over the shared configurations fetch with the master
// enable toggle. Toggling persists the provider enable key with a
// success notice; unlike the landing toggles there is no last-method
// guard. The toggle locks while its save flies or before configuration
// loads, and the form mounts once the map arrives. Credential field
// sets pass straight through to the form (NEWFRONT-511).
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { isConfigFlagOn, toConfigMap } from "../authMethods.js";
import { useInstanceConfigurations, useUpdateInstanceConfigurations } from "../hooks.js";
import {
  PROVIDER_MASTER_FAILED_MESSAGE,
  PROVIDER_MASTER_FAILED_TITLE,
  PROVIDER_MASTER_SAVED_TITLE,
  providerMasterSavedMessage,
  type AuthProviderDef,
  type ProviderCredentialField,
} from "../providerAuth.js";
import { AdminAuthProviderForm, type ProviderCopyHelp } from "./AdminAuthProviderForm.js";
import { AdminAuthProviderShell } from "./AdminAuthProviderShell.js";

export interface AdminAuthProviderPageProps {
  provider: AuthProviderDef;
  /** Credential inputs the form renders (field sets arrive in NEWFRONT-511). */
  fields?: readonly ProviderCredentialField[] | undefined;
  /** Stored-value prefill per key. */
  prefill?: ((key: string, stored: string) => string) | undefined;
  /** Copy-field help copy keyed by field label. */
  copyDescriptions?: Partial<Record<string, React.ReactNode>> | undefined;
  /** Copy-field help links keyed by field label. */
  copyHelp?: Partial<Record<string, ProviderCopyHelp>> | undefined;
}

export function AdminAuthProviderPage({
  provider,
  fields = [],
  prefill,
  copyDescriptions,
  copyHelp,
}: AdminAuthProviderPageProps): React.ReactElement {
  const configurations = useInstanceConfigurations();
  const save = useUpdateInstanceConfigurations();
  const config = React.useMemo(
    () => (configurations.data === undefined ? null : toConfigMap(configurations.data)),
    [configurations.data]
  );

  const flip = (next: boolean): void => {
    save.mutate(
      { [provider.enableKey]: next ? "1" : "0" },
      {
        onSuccess: () => {
          notifySuccess(PROVIDER_MASTER_SAVED_TITLE, providerMasterSavedMessage(provider.name, next));
        },
        onError: () => {
          notifyError(PROVIDER_MASTER_FAILED_TITLE, PROVIDER_MASTER_FAILED_MESSAGE);
        },
      }
    );
  };

  return (
    <AdminAuthProviderShell
      provider={provider}
      enabled={config === null ? false : isConfigFlagOn(config[provider.enableKey])}
      disabled={config === null || save.isPending}
      onToggle={flip}
      ready={config !== null}
    >
      {config === null ? null : (
        <AdminAuthProviderForm
          provider={provider}
          config={config}
          fields={fields}
          prefill={prefill}
          copyDescriptions={copyDescriptions}
          copyHelp={copyHelp}
        />
      )}
    </AdminAuthProviderShell>
  );
}
