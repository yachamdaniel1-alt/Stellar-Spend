/**
 * Base Webhook Handler
 * 
 * Abstract implementation of WebhookHandler with common functionality.
 * Extend this class to implement specific webhook handlers.
 */

import { WebhookHandler, WebhookEvent, WebhookProcessResult } from '../core/interfaces';
import { logger } from '@/lib/logger';

export abstract class BaseWebhookHandler implements WebhookHandler {
  abstract eventType: string;
  abstract priority: number;
  
  /**
   * Process the webhook event
   */
  abstract process(event: WebhookEvent): Promise<WebhookProcessResult>;
  
  /**
   * Validate event data before processing
   */
  protected validateEvent(event: WebhookEvent): boolean {
    if (!event.event) {
      throw new Error('Missing event type');
    }
    
    if (!event.data) {
      throw new Error('Missing event data');
    }
    
    if (event.event !== this.eventType) {
      throw new Error(`Handler expects event type '${this.eventType}', got '${event.event}'`);
    }
    
    return true;
  }
  
  /**
   * Log handler execution
   */
  protected logExecution(event: WebhookEvent, result: WebhookProcessResult, startTime: number): void {
    const duration = Date.now() - startTime;
    
    const logData = {
      eventType: event.event,
      handler: this.constructor.name,
      duration,
      success: result.success,
      transactionId: result.transactionId,
      error: result.error,
    };
    
    if (result.success) {
      logger.info('webhook.handler.completed', logData);
    } else {
      logger.error('webhook.handler.failed', logData);
    }
  }
  
  /**
   * Extract common data from webhook event
   */
  protected extractCommonData(event: WebhookEvent): {
    orderId?: string;
    transactionId?: string;
    amount?: number;
    currency?: string;
    status?: string;
  } {
    const data = event.data || {};
    
    return {
      orderId: data.id || data.orderId,
      transactionId: data.transactionId || data.transaction_id,
      amount: typeof data.amount === 'number' ? data.amount : undefined,
      currency: data.currency,
      status: data.status,
    };
  }
}