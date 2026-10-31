import { useEffect, useState } from 'react';
import type { Theme } from './palette';

const dark = () => window.matchMedia('(prefers-color-scheme: dark)');

// The theme follows the document: an explicit data-theme wins over the OS.
function currentTheme(): Theme {
  const set = document.documentElement.dataset['theme'];
  if (set === 'light' || set === 'dark') return set;
  return dark().matches ? 'dark' : 'light';
}

/** The theme in force, which changes with the OS setting or the document's data-theme. */
export function useTheme(): Theme {
  const [theme, setTheme] = useState(currentTheme);
  useEffect(() => {
    const update = () => {
      setTheme(currentTheme());
    };
    const media = dark();
    media.addEventListener('change', update);
    const watcher = new MutationObserver(update);
    watcher.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });
    return () => {
      media.removeEventListener('change', update);
      watcher.disconnect();
    };
  }, []);
  return theme;
}
