import type { KeyboardEvent, MouseEvent, ReactNode } from "react";
import { openExternalUrl } from "../lib/openExternal";

type Props = {
  readonly href: string;
  readonly className?: string;
  readonly title?: string;
  readonly children?: ReactNode;
  /** Call when open fails (optional toast). */
  readonly onError?: (msg: string) => void;
};

/**
 * Opens `href` in the system default browser.
 * Uses a span (not button) so it can sit inside other buttons without invalid nesting.
 * Stops propagation so parent row clicks are not triggered.
 */
export function ExternalLink({ href, className, title, children, onError }: Props) {
  const open = async (e: MouseEvent | KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    try {
      await openExternalUrl(href);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      onError?.(`打开链接失败：${msg}`);
    }
  };

  return (
    <span
      role="link"
      tabIndex={0}
      className={className ?? "cursor-pointer text-accent hover:underline"}
      title={title ?? `在浏览器中打开 ${href}`}
      onClick={(e) => void open(e)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") void open(e);
      }}
    >
      {children ?? href}
    </span>
  );
}
