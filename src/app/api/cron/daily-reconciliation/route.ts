import { NextRequest, NextResponse } from 'next/server';
import { runDailyReconciliation } from '@/lib/reconciliation';
import { logger } from '@/lib/logger';
import { ErrorHandler } from '@/lib/error-handler';
import { ApiError, ErrorType } from '@/lib/error-types';

export async function POST(req: NextRequest) {
  try {
    const secret = req.headers.get('x-cron-secret');
    if (secret !== process.env.CRON_SECRET) {
      return ErrorHandler.unauthorized('Unauthorized');
    }

    logger.info('cron.daily-reconciliation.start', {});

    const result = await runDailyReconciliation();

    logger.info('cron.daily-reconciliation.complete', {
      runId: result.runId,
      totalTransactions: result.totalTransactions,
      discrepancies: result.discrepancies,
    });

    return NextResponse.json(result);
  } catch (err) {
    logger.error('cron.daily-reconciliation.failed', {}, err);
    return ErrorHandler.handle(new ApiError(ErrorType.SERVER_ERROR, 'Daily reconciliation failed'));
  }
}
