import { createWebhookSystem } from '@/lib/webhook';

export interface WebhookPayload {
  event: string;
  data: {
    id?: string;
    orderId?: string;
    [key: string]: unknown;
  };
}

export interface WebhookProcessResult {
  success: boolean;
  transactionId?: string;
  error?: string;
}

/**
 * Legacy Webhook Service - Maintained for backward compatibility
 * 
 * This service now delegates to the new modular webhook system.
 * New code should use the modular system directly.
 */
export class WebhookService {
  private webhookSystem = createWebhookSystem();

  /**
   * Process Paycrest webhook (legacy method)
   * 
   * @deprecated Use the modular webhook system directly
   */
  async processPaycrestWebhook(payload: WebhookPayload): Promise<WebhookProcessResult> {
    try {
      // Validate payload
      this.validatePayload(payload);

      // Convert to webhook event format
      const webhookEvent = {
        event: payload.event,
        data: payload.data,
        timestamp: Date.now(),
      };

      // Process through modular system
      const results = await this.webhookSystem.processWebhook('paycrest', JSON.stringify(webhookEvent), {});

      // Find successful result with transaction ID
      const successfulResult = results.find(r => r.success && r.transactionId);
      
      if (successfulResult) {
        return {
          success: true,
          transactionId: successfulResult.transactionId,
        };
      }

      // Find any error
      const errorResult = results.find(r => r.error);
      return {
        success: false,
        error: errorResult?.error || 'Failed to process webhook',
      };
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  /**
   * Process webhook using the new modular system
   */
  async processWebhook(
    providerId: string,
    rawBody: string,
    headers: Record<string, string>,
  ): Promise<WebhookProcessResult[]> {
    return this.webhookSystem.processWebhook(providerId, rawBody, headers);
  }

  /**
   * Validate webhook signature using the new modular system
   */
  async validateWebhook(
    providerId: string,
    rawBody: string,
    signature: string,
    headers: Record<string, string>,
  ): Promise<{ valid: boolean; reason?: string }> {
    return this.webhookSystem.validateWebhook(providerId, rawBody, signature, headers);
  }

  /**
   * Get the underlying webhook factory for advanced usage
   */
  getWebhookFactory() {
    return this.webhookSystem;
  }

  private validatePayload(payload: WebhookPayload): void {
    if (!payload || typeof payload !== 'object') {
      throw new Error('Invalid webhook payload');
    }

    if (!payload.event || typeof payload.event !== 'string') {
      throw new Error('Event type is required');
    }

    if (!payload.data || typeof payload.data !== 'object') {
      throw new Error('Webhook data is required');
    }
  }
}
