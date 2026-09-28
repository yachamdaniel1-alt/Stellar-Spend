import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  reconcileTransaction,
  generateReconciliationReport,
  buildSettlementCsv,
  buildDailySettlementReport,
  generateAlerts,
  runReconciliationJob,
  aggregateRecordsByDay,
  performManualReconciliation,
} from './reconciliation';
import type { ReconciliationRecord, ReconciliationDiscrepancy } from './reconciliation';

vi.mock('@/lib/db/client', () => ({
  pool: {
    query: vi.fn().mockResolvedValue({ rows: [] }),
  },
}));

vi.mock('@/lib/logger', () => ({
  logger: {
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
    withContext: vi.fn().mockReturnThis(),
  },
}));

vi.mock('@/lib/env', () => ({
  env: {
    server: {
      STELLAR_HORIZON_URL: 'https://horizon.test',
      BASE_RPC_URL: 'https://base-rpc.test',
      PAYCREST_API_KEY: 'test-key',
    },
  },
}));

function makeRecord(overrides: Partial<ReconciliationRecord> = {}): ReconciliationRecord {
  return {
    transactionId: `tx-${Date.now()}`,
    stellarTxHash: 'stellar-hash-abc',
    baseTxHash: '0xbasehash123',
    paycrestOrderId: 'order-789',
    amount: '100.00',
    currency: 'USDC',
    timestamp: new Date().toISOString(),
    ...overrides,
  };
}

describe('reconcileTransaction', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('detects missing Stellar transaction', async () => {
    global.fetch = vi.fn().mockResolvedValue({ ok: false });

    const record = makeRecord();
    const results = await reconcileTransaction(record);
    expect(results.some((r) => r.type === 'missing_stellar')).toBe(true);
    expect(results.some((r) => r.type === 'missing_base')).toBe(true);
    expect(results.some((r) => r.type === 'missing_paycrest')).toBe(true);
  });

  it('detects amount mismatch when Stellar and Paycrest disagree', async () => {
    global.fetch = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ successful: true }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ result: { hash: '0xabc' } }) })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ data: { status: 'completed', amount: '200.00' } }),
      });

    const record = makeRecord({ amount: '100.00' });
    const results = await reconcileTransaction(record);
    expect(results.some((r) => r.type === 'amount_mismatch')).toBe(true);
  });

  it('detects status mismatch between Stellar and Paycrest', async () => {
    global.fetch = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ successful: false }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ result: { hash: '0xabc' } }) })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ data: { status: 'completed', amount: '100.00' } }),
      });

    const record = makeRecord({ amount: '100.00' });
    const results = await reconcileTransaction(record);
    expect(results.some((r) => r.type === 'status_mismatch')).toBe(true);
  });

  it('detects unsettled Paycrest order', async () => {
    global.fetch = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ successful: true }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ result: { hash: '0xabc' } }) })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ data: { status: 'pending', amount: '100.00' } }),
      });

    const record = makeRecord({ amount: '100.00' });
    const results = await reconcileTransaction(record);
    expect(results.some((r) => r.type === 'unsettled_order')).toBe(true);
  });

  it('returns empty discrepancies for well-matched records', async () => {
    global.fetch = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ successful: true }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ result: { hash: '0xabc' } }) })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({ data: { status: 'completed', amount: '100.00' } }),
      });

    const record = makeRecord({ amount: '100.00' });
    const results = await reconcileTransaction(record);
    expect(results.length).toBe(0);
  });
});

describe('generateReconciliationReport', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('generates a report with correct summary', async () => {
    global.fetch = vi.fn().mockResolvedValue({ ok: false });

    const records = [
      makeRecord({
        transactionId: 'tx-1',
        stellarTxHash: 'hash-1',
        baseTxHash: '0xbase1',
        paycrestOrderId: 'order-1',
      }),
      makeRecord({
        transactionId: 'tx-2',
        stellarTxHash: 'hash-2',
        baseTxHash: '0xbase2',
        paycrestOrderId: 'order-2',
      }),
    ];

    const report = await generateReconciliationReport(records);
    expect(report.totalTransactions).toBe(2);
    expect(report.summary.missingStellar).toBeGreaterThan(0);
    expect(report.summary.missingBase).toBeGreaterThan(0);
    expect(report.summary.missingPaycrest).toBeGreaterThan(0);
    expect(report.date).toBeDefined();
  });

  it('handles empty records', async () => {
    const report = await generateReconciliationReport([]);
    expect(report.totalTransactions).toBe(0);
    expect(report.matchedTransactions).toBe(0);
    expect(report.discrepancies.length).toBe(0);
  });
});

describe('buildSettlementCsv', () => {
  it('generates CSV with header and rows', () => {
    const discrepancies: ReconciliationDiscrepancy[] = [
      {
        transactionId: 'tx-1',
        type: 'missing_stellar',
        description: 'Stellar tx not found',
        severity: 'high',
      },
    ];

    const report = {
      timestamp: new Date().toISOString(),
      date: '2026-06-26',
      totalTransactions: 1,
      matchedTransactions: 0,
      discrepancies,
      summary: {
        missingStellar: 1,
        missingBase: 0,
        missingPaycrest: 0,
        amountMismatches: 0,
        statusMismatches: 0,
        unsettledOrders: 0,
      },
    };

    const csv = buildSettlementCsv(report);
    expect(csv).toContain('Transaction ID');
    expect(csv).toContain('tx-1');
    expect(csv).toContain('missing_stellar');
  });
});

describe('buildDailySettlementReport', () => {
  it('builds a daily settlement report with volumes', async () => {
    vi.restoreAllMocks();
    global.fetch = vi.fn().mockResolvedValue({ ok: false });

    const records = [
      makeRecord({ transactionId: 'tx-daily-1', amount: '100' }),
      makeRecord({ transactionId: 'tx-daily-2', amount: '200' }),
    ];

    const daily = await buildDailySettlementReport(records);
    expect(daily.date).toBeDefined();
    expect(daily.totalCount).toBe(2);
    expect(daily.stellarVolume).toBeDefined();
    expect(daily.paycrestVolume).toBeDefined();
    expect(daily.unsettledOrders).toBeDefined();
  });
});

describe('generateAlerts', () => {
  it('generates high severity alert for missing stellar transactions', () => {
    const discrepancies: ReconciliationDiscrepancy[] = Array.from({ length: 10 }, (_, i) => ({
      transactionId: `tx-${i}`,
      type: 'missing_stellar' as const,
      description: 'Missing',
      severity: 'high' as const,
    }));

    const report = {
      timestamp: new Date().toISOString(),
      date: '2026-06-26',
      totalTransactions: 10,
      matchedTransactions: 0,
      discrepancies,
      summary: {
        missingStellar: 10,
        missingBase: 0,
        missingPaycrest: 0,
        amountMismatches: 0,
        statusMismatches: 0,
        unsettledOrders: 0,
      },
    };

    const alerts = generateAlerts(report);
    expect(alerts.length).toBeGreaterThan(0);
    expect(alerts.some((a) => a.severity === 'high')).toBe(true);
  });

  it('generates alert for unsettled orders over threshold', () => {
    const discrepancies: ReconciliationDiscrepancy[] = Array.from({ length: 5 }, (_, i) => ({
      transactionId: `tx-${i}`,
      type: 'unsettled_order' as const,
      description: 'Unsettled',
      severity: 'medium' as const,
    }));

    const report = {
      timestamp: new Date().toISOString(),
      date: '2026-06-26',
      totalTransactions: 5,
      matchedTransactions: 0,
      discrepancies,
      summary: {
        missingStellar: 0,
        missingBase: 0,
        missingPaycrest: 0,
        amountMismatches: 0,
        statusMismatches: 0,
        unsettledOrders: 5,
      },
    };

    const alerts = generateAlerts(report);
    expect(alerts.some((a) => a.message.includes('unsettled'))).toBe(true);
  });
});

describe('runReconciliationJob', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    global.fetch = vi.fn().mockResolvedValue({ ok: false });
  });

  it('produces a history entry', async () => {
    const records = [makeRecord({ transactionId: 'tx-job-1' })];
    const entry = await runReconciliationJob(records);
    expect(entry.id).toContain('recon_');
    expect(entry.report).toBeDefined();
    expect(entry.alerts).toBeDefined();
  });
});

describe('aggregateRecordsByDay', () => {
  it('groups records by day', async () => {
    const records = [
      makeRecord({ transactionId: 'tx-day-1', timestamp: '2026-06-26T10:00:00Z' }),
      makeRecord({ transactionId: 'tx-day-2', timestamp: '2026-06-26T12:00:00Z' }),
      makeRecord({ transactionId: 'tx-day-3', timestamp: '2026-06-25T08:00:00Z' }),
    ];

    const byDay = await aggregateRecordsByDay([], [], [], records);
    expect(byDay['2026-06-26']).toHaveLength(2);
    expect(byDay['2026-06-25']).toHaveLength(1);
  });
});

describe('performManualReconciliation', () => {
  it('records retry action', async () => {
    const result = await performManualReconciliation({
      transactionId: 'tx-manual-1',
      action: 'retry',
      notes: 'Manual retry requested',
      resolvedBy: 'admin',
    });
    expect(result.success).toBe(true);
  });

  it('records investigate action', async () => {
    const result = await performManualReconciliation({
      transactionId: 'tx-manual-2',
      action: 'investigate',
    });
    expect(result.success).toBe(true);
    expect(result.message).toContain('investigate');
  });
});


describe('fetchDailyRecords', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('returns empty array when dal.getByUser fails', async () => {
    const { fetchDailyRecords } = await import('./reconciliation');
    
    // Mock the dal module
    vi.doMock('@/lib/db', () => ({
      dal: {
        getByUser: vi.fn().mockRejectedValue(new Error('DB error')),
      },
    }));

    const records = await fetchDailyRecords();
    expect(records).toEqual([]);
  });

  it('filters records from yesterday', async () => {
    const { fetchDailyRecords } = await import('./reconciliation');
    
    const mockTransactions = [
      { id: 'tx-1', stellarTxHash: 'hash1', baseTxHash: '0x1', payoutOrderId: 'order1', amount: '100', currency: 'USDC', timestamp: Date.now() - 12 * 60 * 60 * 1000 }, // 12 hours ago
      { id: 'tx-2', stellarTxHash: 'hash2', baseTxHash: '0x2', payoutOrderId: 'order2', amount: '200', currency: 'USDC', timestamp: Date.now() - 48 * 60 * 60 * 1000 }, // 2 days ago - should be filtered out
      { id: 'tx-3', stellarTxHash: 'hash3', baseTxHash: '0x3', payoutOrderId: 'order3', amount: '300', currency: 'USDC', timestamp: Date.now() - 6 * 60 * 60 * 1000 }, // 6 hours ago
    ];

    vi.doMock('@/lib/db', () => ({
      dal: {
        getByUser: vi.fn().mockResolvedValue(mockTransactions),
      },
    }));

    const records = await fetchDailyRecords();
    expect(records).toHaveLength(2); // Only tx-1 and tx-3
    expect(records.map(r => r.transactionId)).toEqual(['tx-1', 'tx-3']);
  });
});

describe('runDailyReconciliation', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('runs daily reconciliation and returns result', async () => {
    const { runDailyReconciliation } = await import('./reconciliation');
    
    // Mock fetchDailyRecords
    vi.doMock('@/lib/db', () => ({
      dal: {
        getByUser: vi.fn().mockResolvedValue([]),
      },
    }));

    const result = await runDailyReconciliation();
    expect(result).toHaveProperty('ok', true);
    expect(result).toHaveProperty('totalTransactions');
    expect(result).toHaveProperty('matchedTransactions');
    expect(result).toHaveProperty('discrepancies');
    expect(result).toHaveProperty('alerts');
    expect(result).toHaveProperty('summary');
    expect(result).toHaveProperty('runId');
    expect(result).toHaveProperty('timestamp');
    expect(result).toHaveProperty('history');
  });
});