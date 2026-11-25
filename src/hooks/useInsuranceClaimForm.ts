import { useCallback, useEffect, useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { apiPatch } from '../lib/api';

export const insuranceClaimSchema = z.object({
  reason: z
    .string()
    .min(1, 'Please provide a reason for the claim')
    .max(1000, 'Reason must be 1000 characters or fewer'),
  evidence: z.string().optional(),
});

export type InsuranceClaimFormValues = z.infer<typeof insuranceClaimSchema>;

export interface UseInsuranceClaimFormOptions {
  transactionId: string;
  insuranceId?: string;
  onSuccess?: () => void;
  onCancel?: () => void;
}

export interface UseInsuranceClaimFormResult {
  form: ReturnType<typeof useForm<InsuranceClaimFormValues>>;
  register: ReturnType<typeof useForm<InsuranceClaimFormValues>>['register'];
  handleSubmit: ReturnType<typeof useForm<InsuranceClaimFormValues>>['handleSubmit'];
  errors: ReturnType<typeof useForm<InsuranceClaimFormValues>>['formState']['errors'];
  isSubmitting: boolean;
  isEligible: boolean;
  isCheckingEligibility: boolean;
  selectedFile: File | null;
  filePreview: string | null;
  submitError: string | null;
  handleFileChange: (event: React.ChangeEvent<HTMLInputElement>) => void;
  handleRemoveFile: () => void;
  handleCancel: () => void;
  onSubmit: (values: InsuranceClaimFormValues) => Promise<void>;
}

const DEMO_TRANSACTION_ID = 'demo';

export function useInsuranceClaimForm({
  transactionId,
  insuranceId,
  onSuccess,
  onCancel,
}: UseInsuranceClaimFormOptions): UseInsuranceClaimFormResult {
  const form = useForm<InsuranceClaimFormValues>({
    resolver: zodResolver(insuranceClaimSchema),
    defaultValues: {
      reason: '',
      evidence: '',
    },
  });

  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
    reset,
  } = form;

  const [isEligible, setIsEligible] = useState(false);
  const [isCheckingEligibility, setIsCheckingEligibility] = useState(false);
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [filePreview, setFilePreview] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const fileReaderRef = useRef<FileReader | null>(null);

  // Eligibility check keyed on insuranceId
  useEffect(() => {
    let cancelled = false;

    if (!insuranceId) {
      setIsEligible(false);
      setIsCheckingEligibility(false);
      return () => {
        cancelled = true;
      };
    }

    setIsCheckingEligibility(true);
    setIsEligible(false);

    // Simulated eligibility resolution; real implementation may call an API.
    Promise.resolve(true)
      .then((eligible) => {
        if (!cancelled) {
          setIsEligible(eligible);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setIsEligible(false);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsCheckingEligibility(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [insuranceId]);

  const handleCancel = useCallback(() => {
    if (onCancel) {
      onCancel();
    }
  }, [onCancel]);

  // Escape-key cancel listener
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        handleCancel();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [handleCancel]);

  const handleFileChange = useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0] ?? null;

    if (fileReaderRef.current) {
      fileReaderRef.current.onload = null;
      fileReaderRef.current.onerror = null;
      if (fileReaderRef.current.readyState === FileReader.LOADING) {
        fileReaderRef.current.abort();
      }
      fileReaderRef.current = null;
    }

    setSelectedFile(file);
    setFilePreview(null);

    if (!file) {
      return;
    }

    const reader = new FileReader();
    fileReaderRef.current = reader;

    reader.onload = () => {
      if (fileReaderRef.current === reader) {
        setFilePreview(typeof reader.result === 'string' ? reader.result : null);
      }
    };
    reader.onerror = () => {
      if (fileReaderRef.current === reader) {
        setFilePreview(null);
      }
    };

    reader.readAsDataURL(file);
  }, []);

  const handleRemoveFile = useCallback(() => {
    if (fileReaderRef.current) {
      fileReaderRef.current.onload = null;
      fileReaderRef.current.onerror = null;
      if (fileReaderRef.current.readyState === FileReader.LOADING) {
        fileReaderRef.current.abort();
      }
      fileReaderRef.current = null;
    }
    setSelectedFile(null);
    setFilePreview(null);
  }, []);

  // Clean up any in-flight FileReader on unmount
  useEffect(() => {
    return () => {
      if (fileReaderRef.current) {
        fileReaderRef.current.onload = null;
        fileReaderRef.current.onerror = null;
        if (fileReaderRef.current.readyState === FileReader.LOADING) {
          fileReaderRef.current.abort();
        }
        fileReaderRef.current = null;
      }
    };
  }, []);

  const onSubmit = useCallback(
    async (values: InsuranceClaimFormValues) => {
      setSubmitError(null);

      const path =
        transactionId === DEMO_TRANSACTION_ID
          ? `/api/mock/transactions/${transactionId}/insurance`
          : `/api/transactions/${transactionId}/insurance`;

      try {
        await apiPatch(path, {
          reason: values.reason,
          evidence: values.evidence ?? '',
          fileName: selectedFile?.name ?? null,
        });

        reset();
        handleRemoveFile();
        if (onSuccess) {
          onSuccess();
        }
      } catch (error) {
        const message =
          error instanceof Error && error.message
            ? error.message
            : 'Failed to submit insurance claim';
        setSubmitError(message);
      }
    },
    [transactionId, selectedFile, reset, handleRemoveFile, onSuccess],
  );

  return {
    form,
    register,
    handleSubmit,
    errors,
    isSubmitting,
    isEligible,
    isCheckingEligibility,
    selectedFile,
    filePreview,
    submitError,
    handleFileChange,
    handleRemoveFile,
    handleCancel,
    onSubmit,
  };
}


