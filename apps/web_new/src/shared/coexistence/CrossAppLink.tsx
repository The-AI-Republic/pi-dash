// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Cross-app link (F-09). Screens that have not migrated yet live in the
// old app behind the same proxy, so they render as plain anchors (a
// full-page load keeps the session cookie on the same origin). Migrated
// screens render as router links for client-side navigation. Both read
// the same migrated-route list, so a link flips the moment its prefix
// joins the list — no call site changes.

import { Link } from "@tanstack/react-router";
import * as React from "react";

import { MIGRATED_ROUTE_PREFIXES, resolveLinkKind } from "./migrated-routes.js";

export interface CrossAppLinkProps extends Omit<React.AnchorHTMLAttributes<HTMLAnchorElement>, "href"> {
  /** App path such as "/sign-in" or "/acme/projects/1/issues". */
  to: string;
  /** Prefix override for tests and previews; defaults to the bundled list. */
  prefixes?: readonly string[] | undefined;
  children: React.ReactNode;
}

// The installed router's prop types are anchored on its own copies of the
// shared UI types (pnpm installs several), which are structurally
// incompatible with the app's copies even though the runtime React is a
// single copy. Props cross that boundary in exactly one place, asserted
// here — never widened, never spread piecemeal.
type RouterLinkProps = Parameters<typeof Link>[0];

export function CrossAppLink({ to, prefixes, children, ...rest }: CrossAppLinkProps): React.ReactElement {
  if (resolveLinkKind(to, prefixes ?? MIGRATED_ROUTE_PREFIXES) === "anchor") {
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  }
  const linkProps = { to, ...rest, children } as RouterLinkProps;
  return <Link {...linkProps} />;
}
