import { useCallback, useEffect, useRef, useState } from 'react';
import {
  KYCLimitService,
  KYCStatus,
  KYCLimits,
  KYCTier,
  KYCSubmissionResult,
  LimitIncreaseRequestResult,
} from '../services/KYCLimitService';

export interface UseKYCLimitsResult {
  kycStatus: KYCStatus | null;
  limits: KYCLimits | null;
  isLoading: boolean;
  error: Error | null;
  submitKYC: (documentType: string, documentId: string) => Promise<KYCSubmissionResult>;
  requestLimitIncrease: (tier: KYCTier) => Promise<LimitIncreaseRequestResult>;
  refetch: () => Promise<void>;
}

const toError = (err: unknown): Error => {
  if (err instanceof Error) {
    return err;
  }
  return new Error(typeof err === 'string' ? err : 'Unknown error');
};

export const useKYCLimits = (userId: string | null | undefined): UseKYCLimitsResult => {
  const [kycStatus, setKycStatus] = useState<KYCStatus | null>(null);
  const [limits, setLimits] = useState<KYCLimits | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [error, setError] = useState<Error | null>(null);

  // Track the latest request to avoid state updates from stale responses.
  const requestIdRef = useRef<number>(0);
  const isMountedRef = useRef<boolean>(true);

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  const load = useCallback(async (): Promise<void> => {
    if (!userId) {
      setKycStatus(null);
      setLimits(null);
      setError(null);
      setIsLoading(false);
      return;
    }

    const requestId = ++requestIdRef.current;
    setIsLoading(true);
    setError(null);

    try {
      const [status, fetchedLimits] = await Promise.all([
        KYCLimitService.getKYCStatus(userId),
        KYCLimitService.getUserLimits(userId),
      ]);

      let resolvedLimits = fetchedLimits;
      if (!resolvedLimits) {
        resolvedLimits = await KYCLimitService.initializeLimits(userId);
      }

      if (!isMountedRef.current || requestId !== requestIdRef.current) {
        return;
      }

      setKycStatus(status);
      setLimits(resolvedLimits);
    } catch (err) {
      if (!isMountedRef.current || requestId !== requestIdRef.current) {
        return;
      }
      setError(toError(err));
    } finally {
      if (isMountedRef.current && requestId === requestIdRef.current) {
        setIsLoading(false);
      }
    }
  }, [userId]);

  useEffect(() => {
    void load();
  }, [load]);

  const refetch = useCallback(async (): Promise<void> => {
    await load();
  }, [load]);

  const submitKYC = useCallback(
    async (documentType: string, documentId: string): Promise<KYCSubmissionResult> => {
      if (!userId) {
        throw new Error('Cannot submit KYC without a user');
      }

      setIsLoading(true);
      setError(null);

      try {
        const result = await KYCLimitService.submitKYC(userId, documentType, documentId);

        if (isMountedRef.current) {
          if (result.status) {
            setKycStatus(result.status);
          }
          if (result.limits) {
            setLimits(result.limits);
          }
        }

        return result;
      } catch (err) {
        const normalized = toError(err);
        if (isMountedRef.current) {
          setError(normalized);
        }
        throw normalized;
      } finally {
        if (isMountedRef.current) {
          setIsLoading(false);
        }
      }
    },
    [userId],
  );

  const requestLimitIncrease = useCallback(
    async (tier: KYCTier): Promise<LimitIncreaseRequestResult> => {
      if (!userId) {
        throw new Error('Cannot request a limit increase without a user');
      }

      setIsLoading(true);
      setError(null);

      try {
        const result = await KYCLimitService.requestLimitIncrease(userId, tier);

        if (isMountedRef.current && result.limits) {
          setLimits(result.limits);
        }

        return result;
      } catch (err) {
        const normalized = toError(err);
        if (isMountedRef.current) {
          setError(normalized);
        }
        throw normalized;
      } finally {
        if (isMountedRef.current) {
          setIsLoading(false);
        }
      }
    },
    [userId],
  );

  return {
    kycStatus,
    limits,
    isLoading,
    error,
    submitKYC,
    requestLimitIncrease,
    refetch,
  };
};


