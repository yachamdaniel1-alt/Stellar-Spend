'use client';

import { useState, useCallback, useEffect, useRef } from 'react';

/**
 * Wallet state for the dashboard. The connection is mocked until Freighter or Lobstr is wired in.
 */
export function useDashboardWallet() {
  const [isConnected, setIsConnected] = useState(false);
  const [isConnecting, setIsConnecting] = useState(false);
  const [walletAddress, setWalletAddress] = useState<string>();
  const [resetKey, setResetKey] = useState(0);

  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current);
        timeoutRef.current = null;
      }
    };
  }, []);

  const handleConnect = useCallback(() => {
    if (timeoutRef.current !== null) {
      clearTimeout(timeoutRef.current);
    }
    setIsConnecting(true);
    // Mock wallet connection
    timeoutRef.current = setTimeout(() => {
      timeoutRef.current = null;
      setIsConnected(true);
      setWalletAddress('GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5');
      setIsConnecting(false);
    }, 1000);
  }, []);

  const handleDisconnect = useCallback(() => {
    if (timeoutRef.current !== null) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
    setIsConnected(false);
    setIsConnecting(false);
    setWalletAddress(undefined);
    setResetKey((prev) => prev + 1);
  }, []);

  return {
    isConnected,
    isConnecting,
    walletAddress,
    resetKey,
    handleConnect,
    handleDisconnect,
    // Backwards-compatible aliases
    connect: handleConnect,
    disconnect: handleDisconnect,
  };
}


