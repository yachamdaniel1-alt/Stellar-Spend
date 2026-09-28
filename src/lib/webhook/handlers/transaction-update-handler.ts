/**
 * Transaction Update Webhook Handler
 * 
 * Handles webhook events that update transaction status.
 * Specifically designed for payment provider webhooks (Paycrest, etc.)
 */

import { BaseWebhookHandler } from './base-handler';
import { WebhookEvent, WebhookProcessResult } from '../core/interfaces';
import { dal } from '@/lib/db';
import { notifyTransactionStatusUpdate } from '@/lib/notifications';
import { logger } from '@/lib/logger';

export interface TransactionUpdateConfig {
  /** Event type to handle */
  eventType: string;
  /** Priority for this handler */
  priority: number;
  /** Mapping of event status to transaction updates */
  statusMapping: Record<string, Record<string, unknown>>;
  /** Provider ID for logging */
  providerId?: string;
}

export class TransactionUpdateHandler extends BaseWebhookHandler {
  readonly eventType: string;
  readonly priority: number;
  
  private config: TransactionUpdateConfig;
  
  constructor(config: TransactionUpdateConfig) {
    super();
    this.config = config;
    this.eventType = config.eventType;
    this.priority = config.priority;
  }
  
  /**
   * Process transaction update webhook
   */
  async process(event: WebhookEvent): Promise<WebhookProcessResult> {
    const startTime = Date.now();
    
    try {
      this.validateEvent(event);
      
      const { orderId, transactionId } = this.extractCommonData(event);
      
      // Find transaction by order ID or transaction ID
      const transaction = await this.findTransaction(orderId, transactionId);
      if (!transaction) {
        const error = `No transaction found for order: ${orderId} or transaction: ${transactionId}`;
        logger.warn('webhook.transaction_not_found', {
          eventType: event.event,
          orderId,
          transactionId,
          providerId: this.config.providerId,
        });
        
        return {
          success: false,
          error,
        };
      }
      
      // Apply status updates based on event type
      const updates = this.config.statusMapping[event.event];
      if (!updates) {
        logger.warn('webhook.no_status_mapping', {
          eventType: event.event,
          transactionId: transaction.id,
        });
        
        return {
          success: true,
          transactionId: transaction.id,
          metadata: { reason: 'No status mapping for this event type' },
        };
      }
      
      // Apply updates to transaction
      await dal.update(transaction.id, updates);
      const updated = await dal.getById(transaction.id);
      
      if (updated) {
        await notifyTransactionStatusUpdate({
          transaction: updated,
          previousStatus: transaction.status,
          previousPayoutStatus: transaction.payoutStatus,
          source: 'webhook',
          providerId: this.config.providerId,
        });
      }
      
      const result: WebhookProcessResult = {
        success: true,
        transactionId: transaction.id,
        metadata: {
          event: event.event,
          updatesApplied: Object.keys(updates),
          providerId: this.config.providerId,
        },
      };
      
      this.logExecution(event, result, startTime);
      return result;
    } catch (error) {
      const result: WebhookProcessResult = {
        success: false,
        error: error instanceof Error ? error.message : String(error),
        metadata: {
          event: event.event,
          error,
          providerId: this.config.providerId,
        },
      };
      
      this.logExecution(event, result, startTime);
      return result;
    }
  }
  
  /**
   * Find transaction by order ID or transaction ID
   */
  private async findTransaction(orderId?: string, transactionId?: string) {
    if (orderId) {
      // Try to find by payout order ID
      const byOrderId = await dal.getByPayoutOrderId(orderId);
      if (byOrderId) {
        return byOrderId;
      }
    }
    
    if (transactionId) {
      // Try to find by transaction ID
      const byTransactionId = await dal.getById(transactionId);
      if (byTransactionId) {
        return byTransactionId;
      }
    }
    
    return null;
  }
  
  /**
   * Get handler configuration
   */
  getConfig(): TransactionUpdateConfig {
    return { ...this.config };
  }
  
  /**
   * Create Paycrest-specific transaction update handler
   */
  static createPaycrestHandler(eventType: string, priority: number = 0): TransactionUpdateHandler {
    const statusMapping: Record<string, Record<string, unknown>> = {
      'payment_order.settled': { status: 'completed', payoutStatus: 'settled' },
      'payment_order.pending': { payoutStatus: 'pending' },
      'payment_order.refunded': { status: 'failed', payoutStatus: 'refunded', error: 'Refunded by Paycrest' },
      'payment_order.expired': { status: 'failed', payoutStatus: 'expired', error: 'Order expired' },
      'payment_order.failed': { status: 'failed', payoutStatus: 'failed', error: 'Payment failed' },
      'payment_order.cancelled': { status: 'cancelled', payoutStatus: 'cancelled', error: 'Order cancelled' },
    };
    
    return new TransactionUpdateHandler({
      eventType,
      priority,
      statusMapping,
      providerId: 'paycrest',
    });
  }
}