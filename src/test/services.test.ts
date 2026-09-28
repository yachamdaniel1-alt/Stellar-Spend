import { describe, it, expect, beforeEach } from 'vitest';
import { QuoteService } from '@/lib/services/quote.service';
import { BridgeService } from '@/lib/services/bridge.service';
import { PayoutService } from '@/lib/services/payout.service';
import { WebhookService } from '@/lib/services/webhook.service';
import { TransactionService } from '@/lib/services/transaction.service';
import { DIContainer } from '@/lib/di/container';
import { configureServices, SERVICE_KEYS, overrideService } from '@/lib/di';

describe('QuoteService', () => {
  let service: QuoteService;

  beforeEach(() => {
    service = new QuoteService();
  });

  it('should validate quote request', async () => {
    await expect(
      service.getQuote({ amount: '', currency: 'NGN', feeMethod: 'USDC' }),
    ).rejects.toThrow('Invalid amount');
  });

  it('should reject unsupported currency', async () => {
    await expect(
      service.getQuote({ amount: '100', currency: 'INVALID', feeMethod: 'USDC' }),
    ).rejects.toThrow('Unsupported currency');
  });

  it('should reject invalid fee method', async () => {
    await expect(
      service.getQuote({ amount: '100', currency: 'NGN', feeMethod: 'INVALID' as 'USDC' | 'XLM' | 'stablecoin' | 'native' }),
    ).rejects.toThrow('feeMethod must be');
  });
});

describe('BridgeService', () => {
  let service: BridgeService;

  beforeEach(() => {
    service = new BridgeService();
  });

  it('should validate build transaction request', async () => {
    await expect(
      service.buildTransaction({
        amount: '',
        fromAddress: 'GABC',
        toAddress: '0x123',
        feePaymentMethod: 'stablecoin',
      }),
    ).rejects.toThrow('Invalid amount');
  });

  it('should reject invalid Stellar address', async () => {
    await expect(
      service.buildTransaction({
        amount: '100',
        fromAddress: 'INVALID',
        toAddress: '0x1234567890123456789012345678901234567890',
        feePaymentMethod: 'stablecoin',
      }),
    ).rejects.toThrow('Invalid Stellar address');
  });

  it('should reject invalid Base address', async () => {
    await expect(
      service.buildTransaction({
        amount: '100',
        fromAddress: 'GABC1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890ABCDE',
        toAddress: 'INVALID',
        feePaymentMethod: 'stablecoin',
      }),
    ).rejects.toThrow('Invalid Base address');
  });

  it('should require XDR and signature for submission', async () => {
    await expect(service.submitTransaction({ xdr: '', signature: '' })).rejects.toThrow(
      'XDR and signature are required',
    );
  });

  it('should require transaction hash for status check', async () => {
    await expect(service.getTransactionStatus('')).rejects.toThrow('Transaction hash is required');
  });
});

describe('PayoutService', () => {
  let service: PayoutService;

  beforeEach(() => {
    service = new PayoutService();
  });

  it('should validate payout request', async () => {
    await expect(
      service.createOrder({
        orderId: '',
        amount: '100',
        currency: 'NGN',
        beneficiary: { institution: 'Bank', accountIdentifier: '123', accountName: 'User' },
        baseAddress: '0x1234567890123456789012345678901234567890',
      }),
    ).rejects.toThrow('Order ID is required');
  });

  it('should reject invalid amount', async () => {
    await expect(
      service.createOrder({
        orderId: 'order_123',
        amount: '0',
        currency: 'NGN',
        beneficiary: { institution: 'Bank', accountIdentifier: '123', accountName: 'User' },
        baseAddress: '0x1234567890123456789012345678901234567890',
      }),
    ).rejects.toThrow('Amount must be a positive number');
  });

  it('should require order ID for status check', async () => {
    await expect(service.getOrderStatus('')).rejects.toThrow('Order ID is required');
  });

  it('should require order ID and amount for payout execution', async () => {
    await expect(service.executePayout('', '')).rejects.toThrow(
      'Order ID and USDC amount are required',
    );
  });
});

describe('WebhookService', () => {
  let service: WebhookService;

  beforeEach(() => {
    service = new WebhookService();
  });

  it('should validate webhook payload', async () => {
    const result = await service.processPaycrestWebhook(null as unknown as Parameters<typeof service.processPaycrestWebhook>[0]);
    expect(result.success).toBe(false);
    expect(result.error).toContain('Invalid webhook payload');
  });

  it('should require event type', async () => {
    const result = await service.processPaycrestWebhook({ event: '', data: {} });
    expect(result.success).toBe(false);
    expect(result.error).toContain('Event type is required');
  });

  it('should require webhook data', async () => {
    const result = await service.processPaycrestWebhook({ event: 'test', data: null as unknown as Record<string, unknown> });
    expect(result.success).toBe(false);
    expect(result.error).toContain('Webhook data is required');
  });

  it('should support new modular webhook processing', async () => {
    const results = await service.processWebhook('paycrest', '{"event":"test","data":{}}', {});
    expect(Array.isArray(results)).toBe(true);
  });

  it('should support webhook validation', async () => {
    const validation = await service.validateWebhook('paycrest', 'test', 'signature', {});
    expect(validation).toHaveProperty('valid');
    expect(validation).toHaveProperty('reason');
  });
});

describe('TransactionService', () => {
  let service: TransactionService;

  beforeEach(() => {
    service = new TransactionService();
  });

  it('should require transaction ID for retrieval', async () => {
    await expect(service.getTransaction('')).rejects.toThrow('Transaction ID is required');
  });

  it('should require order ID for order lookup', async () => {
    await expect(service.getTransactionByPayoutOrderId('')).rejects.toThrow('Order ID is required');
  });

  it('should require transaction ID for update', async () => {
    await expect(service.updateTransaction('', { status: 'completed' })).rejects.toThrow(
      'Transaction ID is required',
    );
  });

  it('should require updates for update operation', async () => {
    await expect(service.updateTransaction('tx_123', {})).rejects.toThrow('No updates provided');
  });

  it('should require transaction ID for deletion', async () => {
    await expect(service.deleteTransaction('')).rejects.toThrow('Transaction ID is required');
  });
});

describe('DIContainer', () => {
  let container: DIContainer;

  beforeEach(() => {
    container = new DIContainer();
    configureServices(container);
  });

  it('should resolve all configured services', async () => {
    const quoteService = await container.resolve(SERVICE_KEYS.QUOTE_SERVICE);
    expect(quoteService).toBeInstanceOf(QuoteService);
    const bridgeService = await container.resolve(SERVICE_KEYS.BRIDGE_SERVICE);
    expect(bridgeService).toBeInstanceOf(BridgeService);
    const payoutService = await container.resolve(SERVICE_KEYS.PAYOUT_SERVICE);
    expect(payoutService).toBeInstanceOf(PayoutService);
    const webhookService = await container.resolve(SERVICE_KEYS.WEBHOOK_SERVICE);
    expect(webhookService).toBeInstanceOf(WebhookService);
    const transactionService = await container.resolve(SERVICE_KEYS.TRANSACTION_SERVICE);
    expect(transactionService).toBeInstanceOf(TransactionService);
  });

  it('should throw error for unregistered service', async () => {
    await expect(container.resolve('nonexistent')).rejects.toThrow('Service not registered');
  });

  it('should return the same singleton instance', async () => {
    const instance1 = await container.resolve(SERVICE_KEYS.QUOTE_SERVICE);
    const instance2 = await container.resolve(SERVICE_KEYS.QUOTE_SERVICE);
    expect(instance1).toBe(instance2);
  });

  it('should support mock overrides', async () => {
    const mockService = { test: 'mock', getQuote: async () => ({}) };
    overrideService(container, SERVICE_KEYS.QUOTE_SERVICE, mockService);
    const resolved = await container.resolve(SERVICE_KEYS.QUOTE_SERVICE);
    expect(resolved).toBe(mockService);
  });

  it('should clear all services', () => {
    container.clear();
    expect(container.has(SERVICE_KEYS.QUOTE_SERVICE)).toBe(false);
  });
});
