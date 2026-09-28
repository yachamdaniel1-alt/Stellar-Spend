import { env } from './env';
import { pool } from './db/client';
import { logger } from './logger';

export interface ReconciliationRecord {
  transactionId: string;
  stellarTxHash?: string;
  baseTxHash?: string;
  paycrestOrderId?: string;
  amount?: string;
  currency?: string;
  timestamp: string;
}

export interface ReconciliationHistoryEntry {
  id: string;
  runAt: string;
  report: ReconciliationReport;
  alerts: ReconciliationAlert[];
}

export interface StellarTxData {
  successful?: boolean;
  [key: string]: unknown;
}

export interface PaycrestOrderData {
  status?: string;
  amount?: unknown;
  senderAmount?: unknown;
  [key: string]: unknown;
}

export type BaseTxData = Record<string, unknown>;

export interface ReconciliationDiscrepancy {
  transactionId: string;
  type:
    | 'missing_stellar'
    | 'missing_base'
    | 'missing_paycrest'
    | 'amount_mismatch'
    | 'status_mismatch'
    | 'unsettled_order';
  description: string;
  severity: 'low' | 'medium' | 'high';
  stellarData?: StellarTxData;
  baseData?: BaseTxData;
  paycrestData?: PaycrestOrderData;
}

export interface ReconciliationReport {
  timestamp: string;
  date: string;
  totalTransactions: number;
  matchedTransactions: number;
  discrepancies: ReconciliationDiscrepancy[];
  summary: {
    missingStellar: number;
    missingBase: number;
    missingPaycrest: number;
    amountMismatches: number;
    statusMismatches: number;
    unsettledOrders: number;
  };
}

export interface ReconciliationAlert {
  severity: 'low' | 'medium' | 'high';
  message: string;
  discrepancies: ReconciliationDiscrepancy[];
  timestamp: string;
}

export interface ManualReconciliationAction {
  transactionId: string;
  action: 'retry' | 'mark_resolved' | 'investigate';
  notes?: string;
  resolvedBy?: string;
}

export interface DailySettlementReport {
  date: string;
  generatedAt: string;
  stellarVolume: string;
  baseVolume: string;
  paycrestVolume: string;
  internalVolume: string;
  matchedCount: number;
  totalCount: number;
  discrepancies: ReconciliationDiscrepancy[];
  unsettledOrders: string[];
  downloadUrl?: string;
}

async function fetchStellarTransaction(txHash: string): Promise<StellarTxData | null> {
  try {
    const response = await fetch(`${env.server.STELLAR_HORIZON_URL}/transactions/${txHash}`, {
      signal: AbortSignal.timeout(5000),
    });
    if (!response.ok) return null;
    return await response.json();
  } catch {
    return null;
  }
}

async function fetchBaseTransaction(txHash: string): Promise<BaseTxData | null> {
  try {
    const response = await fetch(env.server.BASE_RPC_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'eth_getTransactionByHash',
        params: [txHash],
      }),
      signal: AbortSignal.timeout(5000),
    });
    if (!response.ok) return null;
    const data = await response.json();
    return data.result || null;
  } catch {
    return null;
  }
}

async function fetchPaycrestOrder(orderId: string): Promise<PaycrestOrderData | null> {
  try {
    const response = await fetch(`https://api.paycrest.io/aggregator/orders/${orderId}`, {
      headers: { 'x-api-key': env.server.PAYCREST_API_KEY },
      signal: AbortSignal.timeout(5000),
    });
    if (!response.ok) return null;
    const data = await response.json();
    return data.data || null;
  } catch {
    return null;
  }
}

export async function reconcileTransaction(
  record: ReconciliationRecord,
): Promise<ReconciliationDiscrepancy[]> {
  const discrepancies: ReconciliationDiscrepancy[] = [];

  const [stellarData, baseData, paycrestData] = await Promise.all([
    record.stellarTxHash ? fetchStellarTransaction(record.stellarTxHash) : Promise.resolve(null),
    record.baseTxHash ? fetchBaseTransaction(record.baseTxHash) : Promise.resolve(null),
    record.paycrestOrderId ? fetchPaycrestOrder(record.paycrestOrderId) : Promise.resolve(null),
  ]);

  if (record.stellarTxHash && !stellarData) {
    discrepancies.push({
      transactionId: record.transactionId,
      type: 'missing_stellar',
      description: `Stellar transaction ${record.stellarTxHash} not found`,
      severity: 'high',
    });
  }

  if (record.baseTxHash && !baseData) {
    discrepancies.push({
      transactionId: record.transactionId,
      type: 'missing_base',
      description: `Base transaction ${record.baseTxHash} not found`,
      severity: 'high',
    });
  }

  if (record.paycrestOrderId && !paycrestData) {
    discrepancies.push({
      transactionId: record.transactionId,
      type: 'missing_paycrest',
      description: `Paycrest order ${record.paycrestOrderId} not found`,
      severity: 'medium',
    });
  }

  if (paycrestData && paycrestData.status === 'pending') {
    discrepancies.push({
      transactionId: record.transactionId,
      type: 'unsettled_order',
      description: `Paycrest order ${record.paycrestOrderId} is still unsettled`,
      severity: 'medium',
    });
  }

  if (stellarData && paycrestData) {
    const stellarSuccess = stellarData.successful === true;
    const paycrestSuccess = paycrestData.status === 'completed';
    if (stellarSuccess !== paycrestSuccess) {
      discrepancies.push({
        transactionId: record.transactionId,
        type: 'status_mismatch',
        description: 'Status mismatch between Stellar and Paycrest',
        severity: 'high',
        stellarData: { successful: stellarSuccess },
        paycrestData: { status: paycrestData.status },
      });
    }
  }

  if (record.amount && stellarData && paycrestData) {
    const paycrestAmount = paycrestData.amount ?? paycrestData.senderAmount;
    if (paycrestAmount && String(paycrestAmount) !== record.amount) {
      discrepancies.push({
        transactionId: record.transactionId,
        type: 'amount_mismatch',
        description: `Amount mismatch: expected ${record.amount}, Paycrest has ${paycrestAmount}`,
        severity: 'high',
        stellarData,
        paycrestData,
      });
    }
  }

  return discrepancies;
}

export async function generateReconciliationReport(
  records: ReconciliationRecord[],
): Promise<ReconciliationReport> {
  const allDiscrepancies: ReconciliationDiscrepancy[] = [];

  const batchSize = 10;
  for (let i = 0; i < records.length; i += batchSize) {
    const batch = records.slice(i, i + batchSize);
    const batchResults = await Promise.all(batch.map((record) => reconcileTransaction(record)));
    allDiscrepancies.push(...batchResults.flat());
  }

  const uniqueTransactions = new Set(allDiscrepancies.map((d) => d.transactionId));
  const matchedCount = records.length - uniqueTransactions.size;

  const summary = {
    missingStellar: allDiscrepancies.filter((d) => d.type === 'missing_stellar').length,
    missingBase: allDiscrepancies.filter((d) => d.type === 'missing_base').length,
    missingPaycrest: allDiscrepancies.filter((d) => d.type === 'missing_paycrest').length,
    amountMismatches: allDiscrepancies.filter((d) => d.type === 'amount_mismatch').length,
    statusMismatches: allDiscrepancies.filter((d) => d.type === 'status_mismatch').length,
    unsettledOrders: allDiscrepancies.filter((d) => d.type === 'unsettled_order').length,
  };

  return {
    timestamp: new Date().toISOString(),
    date: new Date().toISOString().slice(0, 10),
    totalTransactions: records.length,
    matchedTransactions: matchedCount,
    discrepancies: allDiscrepancies,
    summary,
  };
}

export function generateAlerts(report: ReconciliationReport): ReconciliationAlert[] {
  const alerts: ReconciliationAlert[] = [];

  const highSeverity = report.discrepancies.filter((d) => d.severity === 'high');
  if (highSeverity.length > 0) {
    alerts.push({
      severity: 'high',
      message: `${highSeverity.length} high-severity discrepancies detected`,
      discrepancies: highSeverity,
      timestamp: new Date().toISOString(),
    });
  }

  if (report.summary.missingStellar > 5) {
    alerts.push({
      severity: 'high',
      message: `${report.summary.missingStellar} missing Stellar transactions`,
      discrepancies: report.discrepancies.filter((d) => d.type === 'missing_stellar'),
      timestamp: new Date().toISOString(),
    });
  }

  if (report.summary.missingPaycrest > 5) {
    alerts.push({
      severity: 'medium',
      message: `${report.summary.missingPaycrest} missing Paycrest orders`,
      discrepancies: report.discrepancies.filter((d) => d.type === 'missing_paycrest'),
      timestamp: new Date().toISOString(),
    });
  }

  if (report.summary.unsettledOrders > 3) {
    alerts.push({
      severity: 'medium',
      message: `${report.summary.unsettledOrders} unsettled Paycrest orders`,
      discrepancies: report.discrepancies.filter((d) => d.type === 'unsettled_order'),
      timestamp: new Date().toISOString(),
    });
  }

  return alerts;
}

export function buildSettlementCsv(report: ReconciliationReport): string {
  const header = 'Transaction ID,Discrepancy Type,Severity,Description';
  const rows = report.discrepancies.map(
    (d) => `${d.transactionId},${d.type},${d.severity},"${d.description}"`,
  );
  return [header, ...rows].join('\n');
}

export async function buildDailySettlementReport(
  records: ReconciliationRecord[],
): Promise<DailySettlementReport> {
  const report = await generateReconciliationReport(records);
  const discrepancies = report.discrepancies;

  const stellarVolume = records.filter((r) => r.stellarTxHash).length.toString();
  const baseVolume = records.filter((r) => r.baseTxHash).length.toString();
  const paycrestVolume = records.filter((r) => r.paycrestOrderId).length.toString();
  const unsettled = discrepancies
    .filter((d) => d.type === 'unsettled_order')
    .map((d) => d.transactionId);

  return {
    date: report.date,
    generatedAt: report.timestamp,
    stellarVolume,
    baseVolume,
    paycrestVolume,
    internalVolume: records.length.toString(),
    matchedCount: report.matchedTransactions,
    totalCount: report.totalTransactions,
    discrepancies,
    unsettledOrders: unsettled,
  };
}

export async function performManualReconciliation(
  action: ManualReconciliationAction,
): Promise<{ success: boolean; message: string }> {
  logger.info('reconciliation.manual_action', {
    transactionId: action.transactionId,
    action: action.action,
    notes: action.notes,
    resolvedBy: action.resolvedBy,
  });

   if (action.action === 'retry') {
    const txModule = await import('./transaction-timeout');
    try {
      await txModule.attemptTimeoutRecovery(action.transactionId);
      return {
        success: true,
        message: `Manual retry initiated for transaction ${action.transactionId}`,
      };
    } catch (err) {
      return {
        success: false,
        message: `Manual retry failed: ${err instanceof Error ? err.message : String(err)}`,
      };
    }
  }

  return {
    success: true,
    message: `Manual reconciliation action '${action.action}' recorded for transaction ${action.transactionId}`,
  };
}

export async function createReconciliationTable(): Promise<void> {
  const sql = `
    CREATE TABLE IF NOT EXISTS reconciliation_history (
      id TEXT PRIMARY KEY,
      date TEXT NOT NULL,
      run_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
      report JSONB NOT NULL,
      alerts JSONB NOT NULL DEFAULT '[]'
    )
  `;
  try {
    await pool.query(sql);
  } catch (err) {
    logger.error('reconciliation.create_table_failed', {}, err);
  }
}

export async function storeReconciliationHistory(entry: ReconciliationHistoryEntry): Promise<void> {
  await createReconciliationTable();
  const sql = `
    INSERT INTO reconciliation_history (id, date, run_at, report, alerts)
    VALUES ($1, $2, $3, $4, $5)
    ON CONFLICT (id) DO UPDATE SET
      report = EXCLUDED.report,
      alerts = EXCLUDED.alerts
  `;
  try {
    await pool.query(sql, [
      entry.id,
      entry.report.date,
      entry.runAt,
      JSON.stringify(entry.report),
      JSON.stringify(entry.alerts),
    ]);
    await cleanOldHistory();
  } catch (err) {
    logger.error('reconciliation.store_failed', {}, err);
  }
}

async function cleanOldHistory(): Promise<void> {
  const sql = `DELETE FROM reconciliation_history WHERE run_at < NOW() - INTERVAL '90 days'`;
  try {
    await pool.query(sql);
  } catch {}
}

export async function getReconciliationHistory(): Promise<ReconciliationHistoryEntry[]> {
  await createReconciliationTable();
  const sql = 'SELECT * FROM reconciliation_history ORDER BY run_at DESC LIMIT 30';
  try {
    const result = await pool.query(sql);
    return result.rows.map(
      (row: { id: string; run_at: string | Date; report: ReconciliationReport; alerts: ReconciliationAlert[] }) => ({
        id: row.id,
        runAt: new Date(row.run_at).toISOString(),
        report: row.report,
        alerts: row.alerts,
      }),
    );
  } catch {
    return [];
  }
}

export async function runReconciliationJob(
  records: ReconciliationRecord[],
): Promise<ReconciliationHistoryEntry> {
  const report = await generateReconciliationReport(records);
  const alerts = generateAlerts(report);
  const entry: ReconciliationHistoryEntry = {
    id: `recon_${Date.now()}`,
    runAt: new Date().toISOString(),
    report,
    alerts,
  };

  await storeReconciliationHistory(entry);

  logger.info('reconciliation.job_completed', {
    runId: entry.id,
    totalTransactions: report.totalTransactions,
    discrepancies: report.discrepancies.length,
    alerts: alerts.length,
  });

  return entry;
}

export async function aggregateRecordsByDay(
  stellarTxs: string[],
  baseTxs: string[],
  paycrestOrders: string[],
  internalRecords: ReconciliationRecord[],
): Promise<Record<string, ReconciliationRecord[]>> {
  const byDay: Record<string, ReconciliationRecord[]> = {};

  for (const record of internalRecords) {
    const day = record.timestamp.slice(0, 10);
    if (!byDay[day]) byDay[day] = [];
    byDay[day].push(record);
  }

  return byDay;
}

export async function fetchDailyRecords(): Promise<ReconciliationRecord[]> {
  const yesterday = Date.now() - 24 * 60 * 60 * 1000;
  try {
    // Import dynamically to avoid circular dependency
    const { dal } = await import('@/lib/db');
    const transactions = await dal.getByUser('*').catch(() => []);
    return transactions
      .filter((tx: any) => tx.timestamp >= yesterday)
      .map((tx: any) => ({
        transactionId: tx.id,
        stellarTxHash: tx.stellarTxHash,
        baseTxHash: tx.baseTxHash,
        paycrestOrderId: tx.payoutOrderId,
        amount: tx.amount,
        currency: tx.currency,
        timestamp: new Date(tx.timestamp).toISOString(),
      }));
  } catch {
    return [];
  }
}

export async function runDailyReconciliation(): Promise<any> {
  const records = await fetchDailyRecords();
  const entry = await runReconciliationJob(records);
  const history = await getReconciliationHistory();

  return {
    ok: true,
    totalTransactions: entry.report.totalTransactions,
    matchedTransactions: entry.report.matchedTransactions,
    discrepancies: entry.report.discrepancies.length,
    alerts: entry.alerts.length,
    summary: entry.report.summary,
    runId: entry.id,
    timestamp: entry.runAt,
    history: history.slice(0, 5),
  };
}
