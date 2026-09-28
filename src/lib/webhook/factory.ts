/**
 * Webhook Factory and Configuration System
 * 
 * Provides factory methods for creating and configuring the webhook system.
 */

import { WebhookRegistryImpl } from './core/registry';
import { WebhookProvider, WebhookHandler } from './core/interfaces';
import { PaycrestWebhookProvider, PaycrestWebhookConfig } from './providers/paycrest-provider';
import { TransactionUpdateHandler } from './handlers/transaction-update-handler';
import { logger } from '@/lib/logger';

export interface WebhookSystemConfig {
  /** Paycrest webhook configuration */
  paycrest?: PaycrestWebhookConfig;
  /** Additional providers to register */
  providers?: WebhookProvider[];
  /** Additional handlers to register */
  handlers?: WebhookHandler[];
  /** Whether to register default handlers */
  registerDefaultHandlers?: boolean;
}

export class WebhookFactory {
  private registry: WebhookRegistryImpl;
  
  constructor(private config: WebhookSystemConfig = {}) {
    this.registry = new WebhookRegistryImpl();
    this.initialize();
  }
  
  /**
   * Initialize the webhook system
   */
  private initialize(): void {
    // Register configured providers
    this.registerProviders();
    
    // Register configured handlers
    this.registerHandlers();
    
    // Register default handlers if requested
    if (this.config.registerDefaultHandlers !== false) {
      this.registerDefaultHandlers();
    }
    
    logger.info('webhook.system.initialized', {
      providers: this.registry.getProviderIds(),
      eventTypes: this.registry.getEventTypes(),
    });
  }
  
  /**
   * Register all configured providers
   */
  private registerProviders(): void {
    // Register Paycrest provider if configured
    if (this.config.paycrest) {
      const paycrestProvider = new PaycrestWebhookProvider(this.config.paycrest);
      this.registry.registerProvider(paycrestProvider);
      logger.debug('webhook.provider.registered', {
        providerId: paycrestProvider.providerId,
        displayName: paycrestProvider.displayName,
      });
    }
    
    // Register additional providers
    for (const provider of this.config.providers || []) {
      this.registry.registerProvider(provider);
      logger.debug('webhook.provider.registered', {
        providerId: provider.providerId,
        displayName: provider.displayName,
      });
    }
  }
  
  /**
   * Register all configured handlers
   */
  private registerHandlers(): void {
    for (const handler of this.config.handlers || []) {
      this.registry.registerHandler(handler);
      logger.debug('webhook.handler.registered', {
        eventType: handler.eventType,
        priority: handler.priority,
      });
    }
  }
  
  /**
   * Register default webhook handlers
   */
  private registerDefaultHandlers(): void {
    const defaultHandlers = [
      // Paycrest event handlers
      TransactionUpdateHandler.createPaycrestHandler('payment_order.settled', 0),
      TransactionUpdateHandler.createPaycrestHandler('payment_order.pending', 0),
      TransactionUpdateHandler.createPaycrestHandler('payment_order.refunded', 0),
      TransactionUpdateHandler.createPaycrestHandler('payment_order.expired', 0),
      TransactionUpdateHandler.createPaycrestHandler('payment_order.failed', 0),
      TransactionUpdateHandler.createPaycrestHandler('payment_order.cancelled', 0),
    ];
    
    for (const handler of defaultHandlers) {
      this.registry.registerHandler(handler);
    }
  }
  
  /**
   * Get the webhook registry
   */
  getRegistry(): WebhookRegistryImpl {
    return this.registry;
  }
  
  /**
   * Process webhook for a specific provider
   */
  async processWebhook(
    providerId: string,
    rawBody: string,
    headers: Record<string, string>,
  ) {
    return this.registry.processWebhook(providerId, rawBody, headers);
  }
  
  /**
   * Validate webhook signature for a provider
   */
  async validateWebhook(
    providerId: string,
    rawBody: string,
    signature: string,
    headers: Record<string, string>,
  ): Promise<{ valid: boolean; reason?: string }> {
    const provider = this.registry.getProvider(providerId);
    if (!provider) {
      return { valid: false, reason: `Provider not found: ${providerId}` };
    }
    
    // Special case for Paycrest provider
    if (providerId === 'paycrest' && provider instanceof PaycrestWebhookProvider) {
      const paycrestProvider = provider as PaycrestWebhookProvider;
      return paycrestProvider.validateWebhook(rawBody, signature, headers);
    }
    
    // Generic signature validation for other providers
    try {
      const secret = this.getProviderSecret(providerId);
      if (!secret) {
        return { valid: false, reason: 'No webhook secret configured' };
      }
      
      const isValid = await provider.validateSignature(rawBody, signature, secret);
      return { valid: isValid, reason: isValid ? undefined : 'Invalid signature' };
    } catch (error) {
      return {
        valid: false,
        reason: error instanceof Error ? error.message : String(error),
      };
    }
  }
  
  /**
   * Get webhook secret for a provider
   */
  private getProviderSecret(providerId: string): string | undefined {
    switch (providerId) {
      case 'paycrest':
        return this.config.paycrest?.webhookSecret;
      default:
        return undefined;
    }
  }
  
  /**
   * Create default webhook system configuration
   */
  static createDefaultConfig(env: NodeJS.ProcessEnv = process.env): WebhookSystemConfig {
    return {
      paycrest: {
        webhookSecret: env.PAYCREST_WEBHOOK_SECRET || '',
        timestampToleranceMs: 5 * 60 * 1000, // 5 minutes
        requireNonce: true,
      },
      registerDefaultHandlers: true,
    };
  }
  
  /**
   * Create a webhook system with default configuration
   */
  static createDefault(): WebhookFactory {
    const config = this.createDefaultConfig();
    return new WebhookFactory(config);
  }
  
  /**
   * Create a webhook system from environment variables
   */
  static fromEnvironment(): WebhookFactory {
    const config = this.createDefaultConfig(process.env);
    return new WebhookFactory(config);
  }
  
  /**
   * Check if all required configuration is present
   */
  validateConfiguration(): { valid: boolean; errors: string[] } {
    const errors: string[] = [];
    
    // Check Paycrest configuration
    if (this.config.paycrest) {
      if (!this.config.paycrest.webhookSecret) {
        errors.push('Paycrest webhook secret is required');
      }
    }
    
    return {
      valid: errors.length === 0,
      errors,
    };
  }
}