import { useEffect, useState } from 'react';

/**
 * Returns the current online status of the browser.
 *
 * Subscribes to `navigator.onLine` and the window `online`/`offline` events,
 * updating the returned state whenever connectivity changes. Listeners are
 * cleaned up on unmount.
 *
 * This is the single source of truth for online-status logic referenced by
 * issue #1448 (offline handling consolidation).
 */
export function useOnlineStatus(): boolean {
  const [isOnline, setIsOnline] = useState<boolean>(() => {
    if (typeof navigator === 'undefined' || typeof navigator.onLine !== 'boolean') {
      return true;
    }
    return navigator.onLine;
  });

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    const handleOnline = () => setIsOnline(true);
    const handleOffline = () => setIsOnline(false);

    // Sync with the current value in case it changed between render and effect.
    if (typeof navigator !== 'undefined' && typeof navigator.onLine === 'boolean') {
      setIsOnline(navigator.onLine);
    }

    window.addEventListener('online', handleOnline);
    window.addEventListener('offline', handleOffline);

    return () => {
      window.removeEventListener('online', handleOnline);
      window.removeEventListener('offline', handleOffline);
    };
  }, []);

  return isOnline;
}


