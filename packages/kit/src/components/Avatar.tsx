// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import * as React from "react";
import { cn } from "../lib/cn";

export type AvatarSize = "small" | "medium" | "large";

export interface AvatarProps {
  /** Person the avatar represents; used for initials and naming. */
  name: string;
  src?: string;
  size?: AvatarSize;
  className?: string;
}

const sizeClasses: Record<AvatarSize, string> = {
  small: "size-6 text-caption",
  medium: "size-8 text-meta",
  large: "size-10 text-body",
};

function initials(name: string): string {
  const words = name.trim().split(/\s+/);
  if (words.length === 1) return words[0]!.slice(0, 2).toUpperCase();
  return `${words[0]![0]!}${words[words.length - 1]![0]!}`.toUpperCase();
}

export function Avatar({ name, src, size = "medium", className }: AvatarProps): React.ReactElement {
  const [broken, setBroken] = React.useState(false);
  const showImage = src && !broken;
  return (
    <span
      role="img"
      aria-label={name}
      className={cn(
        "inline-flex shrink-0 items-center justify-center overflow-hidden rounded-(--radius-full) bg-(--subtle) font-medium text-(--text-muted) select-none",
        sizeClasses[size],
        className
      )}
    >
      {showImage ? (
        <img
          src={src}
          alt=""
          aria-hidden="true"
          className="size-full object-cover"
          onError={() => {
            setBroken(true);
          }}
        />
      ) : (
        <span aria-hidden="true">{initials(name)}</span>
      )}
    </span>
  );
}
