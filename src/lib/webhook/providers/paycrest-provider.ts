/**
 * Paycrest Webhook Provider
 * 
 * Implementation of WebhookProvider for Paycrest payment service.
 */

import { BaseWebhookProvider } from '../core/base-provider';
import { WebhookEvent, WebhookProcessResult } from '../core/interfaces';
import { SignatureValidationError } from '../core/errors';
import { logger } from '@/lib/logger';
import { dal } from '@/lib/db';
import { notifyTransactionStatusUpdate } from '@/lib/notifications';

export interface PaycrestWebhookConfig {
  /** Paycrest webhook secret for HMAC validation */
  webhookSecret: string;
  /** Timestamp tolerance in milliseconds (default: 5 minutes) */
  timestampToleranceMs?: number;
  /** Whether to require nonce validation */
  requireNonce?: boolean;
}

export class PaycrestWebhookProvider extends BaseWebhookProvider {
  readonly providerId = 'paycrest';
  readonly displayName = 'Paycrest';
  
  private config: PaycrestWebhookConfig;
  private nonceStore = new Set<string>();
  
  constructor(config: PaycrestWebhookConfig) {
    super();
    this.config = {
      timestampToleranceMs: 5 * 60 * 1000, // 5 minutes default
      requireNonce: true,
      ...config,
    };
  }
  
  /**
   * Validate Paycrest webhook signature using HMAC SHA256
   */
  async validateSignature(payload: string, signature: string, secret: string): Promise<boolean> {
    return this.validateHmacSignature(payload, signature, secret);
  }
  
  /**
   * Parse Paycrest webhook event with specific headers
   */
  async parseEvent(rawBody: string, headers: Record<string, string>): Promise<WebhookEvent> {
    const parsed = await super.parseEvent(rawBody, headers);
    
    // Extract Paycrest-specific headers
    const timestamp = headers['x-paycrest-timestamp'];
    const nonce = headers['x-paycrest-nonce'];
    
    return {
      ...parsed,
      metadata: {
        ...parsed.metadata,
        paycrest: {
          timestamp,
          nonce,
          signature: headers['x-paycrest-signature'],
        },
      },
    };
  }
  
  /**
   * Handle Paycrest webhook event
   */
  async handleEvent(event: WebhookEvent): Promise<WebhookProcessResult> {
    try {
      logger.info('webhook.paycrest.received', {
        event: event.event,
        providerId: this.providerId,
      });
      
      // Map Paycrest event to transaction updates
      const updates = this.mapEventToUpdates(event.event);
      if (!updates) {
        logger.warn('webhook.unhandled_event', {
          event: event.event,
          providerId: this.providerId,
        });
        return {
          success: true,
          metadata: { reason: 'No updates needed for this event type' },
        };
      }
      
      const orderId = event.data?.id ?? event.data?.orderId ?? '';
      if (!orderId) {
        return {
          success: false,
          error: 'No order ID found in webhook data',
        };
      }
      
      const transaction = await dal.getByPayoutOrderId(orderId);
      if (!transaction) {
        return {
          success: false,
          error: `No transaction found for Paycrest order: ${orderId}`,
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
          source: 'paycrest_webhook',
        });
      }
      
      logger.info('webhook.paycrest.processed', {
        event: event.event,
        transactionId: transaction.id,
        providerId: this.providerId,
      });
      
      return {
        success: true,
        transactionId: transaction.id,
        metadata: {
          event: event.event,
          updatesApplied: Object.keys(updates),
        },
      };
    } catch (error) {
      logger.error('webhook.paycrest.error', {
        error: error instanceof Error ? error.message : String(error),
        event: event.event,
        providerId: this.providerId,
      });
      
      return {
        success: false,
        error: error instanceof Error ? error.message : String(error),
        metadata: {
          event: event.event,
          error,
        },
      };
    }
  }
  
  /**
   * Validate Paycrest webhook with timestamp and nonce checks
   */
  async validateWebhook(
    payload: string,
    signature: string,
    headers: Record<string, string>,
  ): Promise<{ valid: boolean; reason?: string }> {
    try {
      // Validate signature
      const secret = this.config.webhookSecret;
      const isValidSignature = await this.validateSignature(payload, signature, secret);
      
      if (!isValidSignature) {
        return { valid: false, reason: 'Invalid signature' };
      }
      
      // Validate timestamp
      const timestamp = headers['x-paycrest-timestamp'];
      if (!timestamp || !this.validateTimestamp(timestamp, this.config.timestampToleranceMs)) {
        return { valid: false, reason: 'Invalid or expired timestamp' };
      }
      
      // Validate nonce if required
      if (this.config.requireNonce) {
        const nonce = headers['x-paycrest-nonce'];
        if (!nonce) {
          return { valid: false, reason: 'Missing nonce' };
        }
        
        const isValidNonce = await this.validateNonceInternal(nonce);
        if (!isValidNonce) {
          return { valid: false, reason: 'Duplicate nonce (replay attack detected)' };
        }
      }
      
      return { valid: true };
    } catch (error) {
      return {
        valid: false,
        reason: error instanceof Error ? error.message : String(error),
      };
    }
  }
  
  /**
   * Map Paycrest event types to transaction updates
   */
  private mapEventToUpdates(eventType: string): Record<string, unknown> | null {
    switch (eventType) {
      case 'payment_order.settled':
        return { status: 'completed', payoutStatus: 'settled' };
      case 'payment_order.pending':
        return { payoutStatus: 'pending' };
      case 'payment_order.refunded':
        return { status: 'failed', payoutStatus: 'refunded', error: 'Refunded by Paycrest' };
      case 'payment_order.expired':
        return { status: 'failed', payoutStatus: 'expired', error: 'Order expired' };
      case 'payment_order.failed':
        return { status: 'failed', payoutStatus: 'failed', error: 'Payment failed' };
      case 'payment_order.cancelled':
        return { status: 'cancelled', payoutStatus: 'cancelled', error: 'Order cancelled' };
      default:
        return null;
    }
  }
  
  /**
   * Internal nonce validation (in-memory store for demo)
   * In production, use a distributed store like Redis
   */
  private async validateNonceInternal(nonce: string): Promise<boolean> {
    if (this.nonceStore.has(nonce)) {
      return false;
    }
    
    this.nonceStore.add(nonce);
    
    // Clean up old nonces periodically (simplified example)
    // In production, use TTL-based storage
    setTimeout(() => {
      this.nonceStore.delete(nonce);
    }, this.config.timestampToleranceMs || 5 * 60 * 1000);
    
    return true;
  }
  
  /**
   * Get provider configuration
   */
  getConfig(): PaycrestWebhookConfig {
    return { ...this.config };
  }
}