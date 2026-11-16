// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
//
// Shared empty-state kit (SHELL-104, NEWFRONT-294): four presentation
// tiers every area reuses as-is — illustrated, marketing, detailed, and
// section. Titles use the --text token and descriptions --text-muted, so
// both inherit the WCAG AA pairs pinned in tokens.test.ts.
import * as React from "react";
import { cn } from "../lib/cn";
import { Button } from "./Button";

export type EmptyStateVariant = "illustrated" | "marketing" | "detailed" | "section";
export type EmptyStateScheme = "light" | "dark" | "high-contrast";

export interface EmptyStateArt {
  category: string;
  type: string;
}

export interface EmptyStateAction {
  label: string;
  onSelect: (event: React.MouseEvent<HTMLButtonElement>) => void;
  disabled?: boolean | undefined;
  loading?: boolean | undefined;
}

/** Resolves a themed illustration to its shipped art path. */
export function emptyStateImagePath(art: EmptyStateArt, scheme: EmptyStateScheme = "light"): string {
  const theme = scheme === "dark" || scheme === "high-contrast" ? "dark" : "light";
  return `/empty-state/${art.category}/${art.type}-${theme}.svg`;
}

interface EmptyStateBase {
  /** Heading text (h3, or a span on the section tier). */
  title: string;
  /** Supporting copy under the heading. */
  description?: React.ReactNode | undefined;
  /**
   * Active scheme for art resolution. Defaults to the resolved document
   * scheme (the data-theme mark), tracked live across theme switches.
   */
  scheme?: EmptyStateScheme | undefined;
  className?: string | undefined;
}

/**
 * Resolve a document theme mark to art. Dark-contrast shares the dark
 * pair; light-contrast shares the light pair. Custom palettes fall back
 * to light: only the app shell knows a custom palette's darkness, so a
 * surface that must render exact art under one keeps passing `scheme`
 * explicitly.
 */
function documentMarkScheme(marked: string | null): EmptyStateScheme {
  return marked === "dark" || marked === "dark-contrast" ? "dark" : "light";
}

/**
 * The resolved document scheme, tracked live. The data-theme mark is the
 * single source: a MutationObserver re-resolves on every applied switch
 * so the art follows without a reload. Hosts without MutationObserver
 * keep the initial read.
 */
function useDocumentScheme(): EmptyStateScheme {
  const [scheme, setScheme] = React.useState<EmptyStateScheme>(() =>
    documentMarkScheme(document.documentElement.getAttribute("data-theme"))
  );
  React.useEffect(() => {
    if (typeof MutationObserver === "undefined") return undefined;
    const root = document.documentElement;
    const refresh = (): void => {
      setScheme(documentMarkScheme(root.getAttribute("data-theme")));
    };
    refresh();
    const observer = new MutationObserver(refresh);
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);
  return scheme;
}

/**
 * Tier props. The art tiers require art (a themed reference renders an
 * img, a node renders inline instead); the section tier takes an icon
 * and one action and never renders art.
 */
export type EmptyStateProps =
  | (EmptyStateBase & {
      variant: "illustrated";
      /** Themed illustration (img), or a custom node (inline art, no img). */
      art: EmptyStateArt | React.ReactNode;
    })
  | (EmptyStateBase & {
      variant: "marketing";
      /** Themed illustration (img), or a custom node (inline art, no img). */
      art: EmptyStateArt | React.ReactNode;
      /** Primary call to action. */
      primaryAction?: EmptyStateAction | undefined;
      /** Hover hint on the primary action. */
      hint?: string | undefined;
    })
  | (EmptyStateBase & {
      variant: "detailed";
      /** Themed illustration (img), or a custom node (inline art, no img). */
      art: EmptyStateArt | React.ReactNode;
      /** Primary call to action. */
      primaryAction?: EmptyStateAction | undefined;
      /** Secondary call to action. */
      secondaryAction?: EmptyStateAction | undefined;
      /** Width cap. */
      size?: "small" | "base" | "large" | undefined;
    })
  | (EmptyStateBase & {
      variant: "section";
      /** Leading node (usually an icon). */
      icon?: React.ReactNode | undefined;
      /** Single action. */
      action?: EmptyStateAction | undefined;
    });

function Artwork({
  art,
  scheme,
  title,
  className,
}: {
  art: EmptyStateArt | React.ReactNode;
  scheme: EmptyStateScheme;
  title: string;
  className?: string | undefined;
}): React.ReactNode {
  if (art === null || art === undefined) return null;
  if (typeof art === "object" && !React.isValidElement(art) && "category" in art && "type" in art) {
    const reference = art as EmptyStateArt;
    return (
      <img
        src={emptyStateImagePath(reference, scheme)}
        alt=""
        aria-hidden="true"
        draggable={false}
        title={title}
        className={className}
      />
    );
  }
  return <div className={className}>{art}</div>;
}

/** Button props for an action, omitting unset flags (exactOptionalPropertyTypes). */
function actionProps(action: EmptyStateAction): {
  onClick: EmptyStateAction["onSelect"];
  disabled?: boolean;
  loading?: boolean;
} {
  return {
    onClick: action.onSelect,
    ...(action.disabled === undefined ? null : { disabled: action.disabled }),
    ...(action.loading === undefined ? null : { loading: action.loading }),
  };
}

function IllustratedBody(
  props: Extract<EmptyStateProps, { variant: "illustrated" }> & { scheme: EmptyStateScheme }
): React.ReactElement {
  return (
    <div className={cn("flex flex-col items-center gap-(--space-6) py-(--space-16)", props.className)}>
      <Artwork art={props.art} scheme={props.scheme} title={props.title} className="h-40 w-40" />
      <h3 className="flex flex-col items-center gap-(--space-2) text-center">
        <span className="text-h3 text-(--text)">{props.title}</span>
        {props.description ? <span className="text-body text-(--text-muted)">{props.description}</span> : null}
      </h3>
    </div>
  );
}

function MarketingBody(
  props: Extract<EmptyStateProps, { variant: "marketing" }> & { scheme: EmptyStateScheme }
): React.ReactElement {
  return (
    <div className={cn("flex flex-col items-center gap-(--space-8) px-(--space-10) py-(--space-20)", props.className)}>
      <Artwork art={props.art} scheme={props.scheme} title={props.title} className="w-60" />
      <h3 className="flex flex-col items-center gap-(--space-2) text-center">
        <span className="text-h3 text-(--text)">{props.title}</span>
        {props.description ? <span className="text-body text-(--text-muted)">{props.description}</span> : null}
      </h3>
      {props.primaryAction ? (
        <Button
          variant="primary"
          {...actionProps(props.primaryAction)}
          {...(props.hint ? { title: props.hint } : null)}
        >
          {props.primaryAction.label}
        </Button>
      ) : null}
    </div>
  );
}

const DETAILED_WIDTHS = { small: "max-w-60", base: "max-w-96", large: "max-w-xl" } as const;

function DetailedBody(
  props: Extract<EmptyStateProps, { variant: "detailed" }> & { scheme: EmptyStateScheme }
): React.ReactElement {
  return (
    <div
      className={cn(
        "flex w-full flex-col items-center gap-(--space-8) px-(--space-6) py-(--space-12)",
        DETAILED_WIDTHS[props.size ?? "base"],
        props.className
      )}
    >
      <h3 className="flex flex-col items-center gap-(--space-2) text-center">
        <span className="text-h3 text-(--text)">{props.title}</span>
        {props.description ? <span className="text-body text-(--text-muted)">{props.description}</span> : null}
      </h3>
      <Artwork art={props.art} scheme={props.scheme} title={props.title} className="h-36 w-36" />
      {props.primaryAction || props.secondaryAction ? (
        <div className="flex items-center gap-(--space-4)">
          {props.primaryAction ? (
            <Button variant="primary" {...actionProps(props.primaryAction)}>
              {props.primaryAction.label}
            </Button>
          ) : null}
          {props.secondaryAction ? (
            <Button {...actionProps(props.secondaryAction)}>{props.secondaryAction.label}</Button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function SectionBody(props: Extract<EmptyStateProps, { variant: "section" }>): React.ReactElement {
  return (
    <div className={cn("flex flex-col gap-(--space-2)", props.className)}>
      <div className="flex items-center gap-(--space-4)">
        {props.icon ?? null}
        <span className="text-emphasis text-(--text)">{props.title}</span>
      </div>
      {props.description ? (
        <span className="text-meta text-(--text-muted)">
          <span>{props.description}</span>
        </span>
      ) : null}
      {props.action ? (
        <div>
          <Button variant="ghost" size="small" {...actionProps(props.action)}>
            {props.action.label}
          </Button>
        </div>
      ) : null}
    </div>
  );
}

/** Shared empty-state presentation. Every area reuses these tiers as-is. */
export function EmptyState(props: EmptyStateProps): React.ReactElement {
  const documentScheme = useDocumentScheme();
  const scheme = props.scheme ?? documentScheme;
  switch (props.variant) {
    case "illustrated":
      return <IllustratedBody {...props} scheme={scheme} />;
    case "marketing":
      return <MarketingBody {...props} scheme={scheme} />;
    case "detailed":
      return <DetailedBody {...props} scheme={scheme} />;
    case "section":
      return <SectionBody {...props} />;
  }
}
