import { describe, it, expect } from 'vitest';
import {
  calculateBridgeFee,
  calculateNetworkFee,
  calculatePaycrestFee,
  calculateTotalFees,
  calculateAllFees,
  calculateAmountAfterFees,
} from '../fee-calculation';
import {
  FEE_CONSTANTS,
  calculateBasisPointsFee,
} from '@stellar-spend/shared';

describe('Fee Calculation Parity Test', () => {
  describe('Backend fee estimates match shared constants', () => {
    it('should calculate bridge fee correctly for stablecoin', () => {
      const amount = '1000';
      const fee = calculateBridgeFee(amount, 'stablecoin');
      const expectedFee = (1000 * FEE_CONSTANTS.STABLECOIN_FEE_PERCENTAGE) / 100;
      expect(parseFloat(fee)).toBeCloseTo(expectedFee, 5);
    });

    it('should not charge bridge fee for native XLM', () => {
      const amount = '1000';
      const fee = calculateBridgeFee(amount, 'native');
      expect(fee).toBe('0');
    });

    it('should calculate network fee correctly for native XLM', () => {
      const fee = calculateNetworkFee('native');
      expect(fee).toBe(FEE_CONSTANTS.NETWORK_FEE_XLM);
    });

    it('should not charge network fee for stablecoin', () => {
      const fee = calculateNetworkFee('stablecoin');
      expect(fee).toBe('0');
    });

    it('should calculate paycrest fee correctly', () => {
      const receiveAmount = '5000';
      const fee = calculatePaycrestFee(receiveAmount);
      const expectedFee = (5000 * FEE_CONSTANTS.PAYCREST_FEE_PERCENTAGE) / 100;
      expect(parseFloat(fee)).toBeCloseTo(expectedFee, 2);
    });

    it('should use basis points consistently', () => {
      const amount = 10_000_000; // 10M stroops
      const stabledFee = calculateBasisPointsFee(amount, FEE_CONSTANTS.STABLECOIN_FEE_BASIS_POINTS);
      const expectedFee = (amount * FEE_CONSTANTS.STABLECOIN_FEE_PERCENTAGE) / 100;
      expect(stabledFee).toBeCloseTo(expectedFee, 2);
    });
  });

  describe('Fee calculations maintain total consistency', () => {
    it('should calculate consistent totals across different amounts', async () => {
      const testCases = [
        { amount: '100', currency: 'USDC' },
        { amount: '1000', currency: 'USDC' },
        { amount: '10000', currency: 'USDC' },
      ];

      for (const testCase of testCases) {
        const breakdown = await calculateAllFees({
          amount: testCase.amount,
          currency: testCase.currency,
          feeMethod: 'stablecoin',
          receiveAmount: testCase.amount,
        });

        const bridgeFee = parseFloat(breakdown.bridgeFee);
        const paycrestFee = parseFloat(breakdown.paycrestFee);
        const totalFee = parseFloat(breakdown.totalFee);

        // Total should be sum of components
        expect(totalFee).toBeCloseTo(bridgeFee + paycrestFee, 5);
      }
    });

    it('should maintain parity between basis-point and percentage calculations', () => {
      const amount = 10_000_000;

      // Using percentage
      const percentageFee = (amount * FEE_CONSTANTS.STABLECOIN_FEE_PERCENTAGE) / 100;

      // Using basis points
      const basisPointsFee = calculateBasisPointsFee(amount, FEE_CONSTANTS.STABLECOIN_FEE_BASIS_POINTS);

      expect(basisPointsFee).toBeCloseTo(percentageFee, 2);
    });
  });

  describe('Edge cases are handled consistently', () => {
    it('should handle zero amounts', async () => {
      const breakdown = await calculateAllFees({
        amount: '0',
        currency: 'USDC',
        feeMethod: 'stablecoin',
        receiveAmount: '0',
      });

      expect(parseFloat(breakdown.bridgeFee)).toBeCloseTo(0, 5);
      expect(parseFloat(breakdown.paycrestFee)).toBeCloseTo(0, 2);
      expect(parseFloat(breakdown.totalFee)).toBeCloseTo(0, 5);
    });

    it('should handle maximum fee constraints', () => {
      // Verify max fee percentage doesn't exceed maximum basis points
      const maxFeeBP = FEE_CONSTANTS.MAX_FEE_BASIS_POINTS;
      const maxFeePercentage = FEE_CONSTANTS.MAX_FEE_PERCENTAGE;

      expect(maxFeePercentage).toBe(maxFeeBP / 100);
    });
  });

  describe('Cross-check with Rust contract truncation toward zero', () => {
    // The Rust contract uses basis_points_of() from stellar_spend_shared
    // which truncates toward zero (i128 division). Frontend must match.

    it('calculateBridgeFee truncates toward zero like Rust basis_points_of', () => {
      // Rust: calculate_fee(&1, &1) → 0.0001 truncates to 0
      // Amount 0.001 USDC at 0.5% = 0.000005, should truncate to 0.000005 (6dp)
      const tinyAmountFee = calculateBridgeFee('0.001', 'stablecoin');
      expect(tinyAmountFee).toBe('0.000005');

      // Rust: calculate_fee(&10_000, &1) → 1
      // Amount 10000 at 50bp (0.5%) = 50, no truncation needed
      const exactFee = calculateBridgeFee('10000', 'stablecoin');
      expect(exactFee).toBe('50.000000');

      // Verify no rounding up occurs (the key parity issue)
      // Amount 0.01 at 0.5% = 0.00005, toFixed(6) rounds to 0.000050
      // But truncation should also give 0.000050 for this case
      // The critical case: where toFixed would round up but truncation should not
      // Amount 0.0015 at 0.5% = 0.0000075, toFixed(6) rounds to 0.000008
      // Truncation gives 0.000007
      const roundingCaseFee = calculateBridgeFee('0.0015', 'stablecoin');
      expect(roundingCaseFee).toBe('0.000007');
    });

    it('calculatePaycrestFee truncates toward zero like Rust basis_points_of', () => {
      // Amount where toFixed(2) would round but truncation should not
      // 0.015 * 1.0% = 0.00015, toFixed(2) → 0.00, truncation → 0.00
      const tinyPaycrest = calculatePaycrestFee('0.015');
      expect(tinyPaycrest).toBe('0.00');

      // 5000 * 1.0% = 50, exact
      const exactPaycrest = calculatePaycrestFee('5000');
      expect(exactPaycrest).toBe('50.00');
    });

    it('calculateTotalFees truncates toward zero', () => {
      const total = calculateTotalFees('0.0000005', '0', '0', 'USDC');
      // 0.0000005 with 6 decimal truncation → 0.000001 (but Math.trunc of 0.5 is 0)
      // Actually: 0.0000005 * 1000000 = 0.5, Math.trunc(0.5) = 0
      expect(total).toBe('0.000000');
    });

    it('calculateAmountAfterFees subtracts totalFee not just bridgeFee', () => {
      const amount = '1000';
      const totalFee = '50.000000';
      const bridgeFee = '5.000000';
      const result = calculateAmountAfterFees(amount, totalFee);
      // Should subtract totalFee (50), not bridgeFee (5)
      expect(parseFloat(result)).toBeCloseTo(950, 5);
    });

    it('calculateAllFees uses totalFee for amountAfterFees', async () => {
      const result = await calculateAllFees({
        amount: '1000',
        currency: 'USDC',
        feeMethod: 'stablecoin',
        receiveAmount: '1000',
      });
      const amountNum = parseFloat(result.amount);
      const afterFeesNum = parseFloat(result.amountAfterFees);
      const totalFeeNum = parseFloat(result.totalFee);
      expect(afterFeesNum).toBeCloseTo(amountNum - totalFeeNum, 5);
    });
  });
});
