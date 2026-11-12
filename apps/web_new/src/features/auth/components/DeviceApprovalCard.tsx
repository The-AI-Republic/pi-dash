// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// CLI device-approval card (AUTH-021, from parity/specs/auth.md). The
// code field autofocuses and auto-formats as typed; a CLI-carried code
// prefills and fires one automatic attempt on mount; approval swaps the
// form for a confirmation naming the account and workspace, while a
// failure keeps the form editable with the server detail.

import { Button, Input } from "@pidash/kit";
import * as React from "react";

import { useApproveDeviceCode } from "../api.js";
import {
  describeDeviceApprovalFailure,
  DEVICE_CODE_FIELD_LENGTH,
  formatDeviceCode,
  isCompleteDeviceCode,
} from "../device.js";
import { authStepInputProps } from "../focus.js";
import { AuthBanner } from "./SignInCard.js";

export interface DeviceApprovalCardProps {
  /** CLI-carried code (?code=): prefills the field and tries once on mount. */
  initialCode?: string | undefined;
}

interface DeviceGrant {
  email: string;
  workspace: string | null;
}

export function DeviceApprovalCard({ initialCode = "" }: DeviceApprovalCardProps): React.ReactElement {
  const [code, setCode] = React.useState(() => formatDeviceCode(initialCode));
  const [failure, setFailure] = React.useState<string | null>(null);
  const [grant, setGrant] = React.useState<DeviceGrant | null>(null);
  const approve = useApproveDeviceCode();
  const autoAttempted = React.useRef(false);

  const submit = React.useCallback(
    (value: string) => {
      setFailure(null);
      approve.mutate(value, {
        onSuccess: (response) => setGrant({ email: response.user_email, workspace: response.workspace_slug }),
        onError: (error) => setFailure(describeDeviceApprovalFailure(error)),
      });
    },
    [approve]
  );

  React.useEffect(() => {
    if (autoAttempted.current) return;
    autoAttempted.current = true;
    const prefilled = formatDeviceCode(initialCode);
    if (isCompleteDeviceCode(prefilled)) submit(prefilled);
  }, [initialCode, submit]);

  if (grant !== null) {
    return (
      <section aria-label="Device approval" className="flex w-full max-w-90 flex-col gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <h1 className="text-h2 text-(--text)">CLI login approved</h1>
          <p className="text-body text-(--text-muted)">The command line will collect its token in a few seconds.</p>
        </div>
        <dl className="text-body flex flex-col gap-(--space-2) text-(--text)">
          <div className="flex items-center justify-between gap-(--space-4)">
            <dt className="text-(--text-muted)">Account</dt>
            <dd>{grant.email}</dd>
          </div>
          {grant.workspace === null ? null : (
            <div className="flex items-center justify-between gap-(--space-4)">
              <dt className="text-(--text-muted)">Workspace</dt>
              <dd>{grant.workspace}</dd>
            </div>
          )}
        </dl>
        <p className="text-body text-(--text-muted)">You can close this page now.</p>
      </section>
    );
  }

  return (
    <section aria-label="Device approval" className="flex w-full max-w-90 flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-1)">
        <h1 className="text-h2 text-(--text)">Approve a CLI login</h1>
        <p className="text-body text-(--text-muted)">
          Type the code printed on your terminal so the command line can sign in to your account.
        </p>
      </div>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (isCompleteDeviceCode(code) && !approve.isPending) submit(code);
        }}
        className="flex flex-col gap-(--space-4)"
      >
        <Input
          label="Device code"
          type="text"
          autoComplete="off"
          spellCheck={false}
          required
          maxLength={DEVICE_CODE_FIELD_LENGTH}
          placeholder="XXXX-YYYY"
          {...authStepInputProps()}
          value={code}
          onChange={(event) => setCode(formatDeviceCode(event.target.value))}
        />
        {failure === null ? null : <AuthBanner message={failure} />}
        <Button
          type="submit"
          variant="primary"
          loading={approve.isPending}
          disabled={!isCompleteDeviceCode(code) || approve.isPending}
        >
          {approve.isPending ? "Approving…" : "Approve"}
        </Button>
      </form>
      <p className="text-body text-(--text-muted)">
        Only approve a code you requested yourself by running <code>pidash auth login</code> on a device you control.
      </p>
    </section>
  );
}
