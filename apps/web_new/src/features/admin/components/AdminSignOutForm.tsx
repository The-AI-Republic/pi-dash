// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin sign-out (ADM-010, from parity/specs/admin.md §1.10): a native
// full-page POST carrying the hidden forgery token. The server clears
// the session and redirects to the mount; the submit handler first
// resets admin client state (theme back to system, fresh stores,
// cleared queries) so the dashboard guard bounces afterwards. The
// account menu (ADM-064) hosts this form as a direct menu child: the
// submitter already carries the menuitem role, so it must not be
// wrapped in another one.
import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { useAdminCsrfToken } from "../hooks.js";
import { resetAdminClientState } from "../stores.js";

export const SIGN_OUT_POST_PATH = "/api/instances/admins/sign-out/";

export function AdminSignOutForm(): React.ReactElement {
  const csrf = useAdminCsrfToken();
  const queryClient = useQueryClient();
  return (
    <form
      method="post"
      action={SIGN_OUT_POST_PATH}
      data-testid="admin-sign-out-form"
      onSubmit={() => resetAdminClientState(queryClient)}
    >
      <input type="hidden" name="csrfmiddlewaretoken" value={csrf.data?.csrf_token ?? ""} readOnly />
      <button
        type="submit"
        role="menuitem"
        data-testid="admin-sign-out"
        className="text-body w-full cursor-pointer px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--subtle)"
      >
        Sign out
      </button>
    </form>
  );
}
