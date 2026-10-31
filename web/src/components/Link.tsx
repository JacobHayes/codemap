import type { ReactNode } from 'react';
import { serializeState, type Navigate, type UrlState } from '../urlState';

type LinkProps = {
  state: UrlState;
  patch: Partial<UrlState>;
  navigate: Navigate;
  className?: string;
  current?: boolean;
  children: ReactNode;
};

/** A real link to a state, so it can be copied or opened in a new tab; a plain click navigates
 * in place. */
export function Link({ state, patch, navigate, className, current, children }: LinkProps) {
  const href = `${window.location.pathname}${serializeState({ ...state, ...patch })}`;
  return (
    <a
      href={href}
      className={className}
      aria-current={current === true ? 'location' : undefined}
      onClick={(e) => {
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
        e.preventDefault();
        navigate(patch);
      }}
    >
      {children}
    </a>
  );
}
