import type { ResourceFeeEstimate } from './stellar/resource-fee-estimator';
import { FEE_CONSTANTS } from '@stellar-spend/shared';

export interface FeeBreakdown {
  bridgeFee: string;
  networkFee: string;
  paycrestFee: string;
  contractResourceFee?: string;
  totalFee: string;
  amount: string;
  amountAfterFees: string;
  currency: string;
}

export interface FeeCalculationParams {
  amount: string;
  currency: string;
  feeMethod: 'stablecoin' | 'native';
  bridgeAmount?: string;
  receiveAmount?: string;
  contractResourceEstimate?: ResourceFeeEstimate;
}

const STABLECOIN_FEE_PERCENTAGE = FEE_CONSTANTS.STABLECOIN_FEE_PERCENTAGE;
const PAYCREST_FEE_PERCENTAGE = FEE_CONSTANTS.PAYCREST_FEE_PERCENTAGE;
const NETWORK_FEE_XLM = FEE_CONSTANTS.NETWORK_FEE_XLM;

function truncateTowardZero(value: number, decimals: number): string {
  const factor = Math.pow(10, decimals);
  return (Math.trunc(value * factor) / factor).toFixed(decimals);
}

export function calculateBridgeFee(amount: string, feeMethod: 'stablecoin' | 'native'): string {
  if (feeMethod === 'native') {
    return '0';
  }

  const amountNum = parseFloat(amount);
  if (isNaN(amountNum) || amountNum <= 0) {
    throw new Error('Invalid amount for fee calculation');
  }

  const fee = (amountNum * STABLECOIN_FEE_PERCENTAGE) / 100;
  return truncateTowardZero(fee, 6);
}

export function calculateNetworkFee(feeMethod: 'stablecoin' | 'native'): string {
  if (feeMethod === 'stablecoin') {
    return '0';
  }
  return NETWORK_FEE_XLM;
}

export function calculatePaycrestFee(receiveAmount: string): string {
  const amountNum = parseFloat(receiveAmount);
  if (isNaN(amountNum) || amountNum <= 0) {
    return '0';
  }

  const fee = (amountNum * PAYCREST_FEE_PERCENTAGE) / 100;
  return truncateTowardZero(fee, 2);
}

export function calculateTotalFees(
  bridgeFee: string,
  networkFee: string,
  paycrestFee: string,
  currency: string,
  contractResourceFee?: string,
): string {
  const bridge = parseFloat(bridgeFee) || 0;
  const network = parseFloat(networkFee) || 0;
  const paycrest = parseFloat(paycrestFee) || 0;
  const contractFee = contractResourceFee ? parseFloat(contractResourceFee) || 0 : 0;

  const total = bridge + network + paycrest + contractFee;
  return truncateTowardZero(total, 6);
}

export function calculateAmountAfterFees(amount: string, totalFee: string): string {
  const amountNum = parseFloat(amount);
  const feeNum = parseFloat(totalFee);

  if (isNaN(amountNum) || isNaN(feeNum)) {
    throw new Error('Invalid amounts for calculation');
  }

  const result = amountNum - feeNum;
  return result > 0 ? result.toFixed(6) : '0';
}

export async function calculateAllFees(params: FeeCalculationParams): Promise<FeeBreakdown> {
  const { amount, currency, feeMethod, receiveAmount, contractResourceEstimate } = params;

  const bridgeFee = calculateBridgeFee(amount, feeMethod);
  const networkFee = calculateNetworkFee(feeMethod);
  const paycrestFee = receiveAmount ? calculatePaycrestFee(receiveAmount) : '0';
  const contractResourceFee = contractResourceEstimate
    ? contractResourceEstimate.estimatedFeeXLM
    : undefined;

  const totalFee = calculateTotalFees(
    bridgeFee,
    networkFee,
    paycrestFee,
    currency,
    contractResourceFee,
  );
  const amountAfterFees = calculateAmountAfterFees(amount, totalFee);

  return {
    bridgeFee,
    networkFee,
    paycrestFee,
    contractResourceFee,
    totalFee,
    amount,
    amountAfterFees,
    currency,
  };
}

export interface DetailedFeeBreakdown extends FeeBreakdown {
  breakdown: {
    bridge: {
      fee: string;
      percentage: string;
      description: string;
    };
    network: {
      fee: string;
      description: string;
    };
    paycrest: {
      fee: string;
      percentage: string;
      description: string;
    };
  };
}

export async function getDetailedFeeBreakdown(
  params: FeeCalculationParams,
): Promise<DetailedFeeBreakdown> {
  const basicFees = await calculateAllFees(params);

  return {
    ...basicFees,
    breakdown: {
      bridge: {
        fee: basicFees.bridgeFee,
        percentage: params.feeMethod === 'stablecoin' ? `${STABLECOIN_FEE_PERCENTAGE}%` : '0%',
        description:
          params.feeMethod === 'stablecoin'
            ? 'Bridge fee for USDC transactions'
            : 'No bridge fee for XLM transactions',
      },
      network: {
        fee: basicFees.networkFee,
        description:
          params.feeMethod === 'native'
            ? 'Stellar network transaction fee'
            : 'No network fee (paid in USDC)',
      },
      paycrest: {
        fee: basicFees.paycrestFee,
        percentage: `${PAYCREST_FEE_PERCENTAGE}%`,
        description: 'Paycrest processing fee',
      },
    },
  };
}
