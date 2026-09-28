/**
 * Modular Webhook System - Main Entry Point
 * 
 * Exports all webhook system components for easy consumption.
 */

// Core abstractions
export {
  WebhookEvent,
  WebhookProvider,
  WebhookHandler,
  WebhookRegistry,
  WebhookProcessResult,
} from './core/interfaces';

export {
  WebhookError,
  SignatureValidationError,
  EventParsingError,
  ProviderNotFoundError,
  HandlerNotFoundError,
  EventProcessingError,
} from './core/errors';

export { BaseWebhookProvider } from './core/base-provider';
export { WebhookRegistryImpl } from './core/registry';

// Providers
export {
  PaycrestWebhookProvider,
  PaycrestWebhookConfig,
} from './providers/paycrest-provider';

// Handlers
export { BaseWebhookHandler } from './handlers/base-handler';
export {
  TransactionUpdateHandler,
  TransactionUpdateConfig,
} from './handlers/transaction-update-handler';

// Factory and Configuration
export {
  WebhookFactory,
  WebhookSystemConfig,
} from './factory';

// Utility modules (re-export existing utilities)
export { default as webhookSecurity } from './security';
export { default as webhookDispatcher } from './dispatcher';
export { default as webhookDeliveryLog } from './delivery-log';
export { default as webhookRetryScheduler } from './retry-scheduler';
export { default as webhookDlq } from './dlq';
export { default as webhookAlertService } from './alert-service';

/**
 * Create and configure the default webhook system
 * 
 * @example
 * ```typescript
 * import { createWebhookSystem } from '@/lib/webhook';
 * 
 * const webhookSystem = createWebhookSystem();
 * const result = await webhookSystem.processWebhook('paycrest', rawBody, headers);
 * ```
 */
export function createWebhookSystem(config?: WebhookSystemConfig): WebhookFactory {
  return new WebhookFactory(config);
}

/**
 * Get the default webhook factory (configured from environment variables)
 */
export function getDefaultWebhookFactory(): WebhookFactory {
  return WebhookFactory.fromEnvironment();
}

/**
 * Process a webhook with the default system
 */
export async function processWebhook(
  providerId: string,
  rawBody: string,
  headers: Record<string, string>,
): Promise<WebhookProcessResult[]> {
  const factory = getDefaultWebhookFactory();
  return factory.processWebhook(providerId, rawBody, headers);
}

/**
 * Validate webhook signature with the default system
 */
export async function validateWebhook(
  providerId: string,
  rawBody: string,
  signature: string,
  headers: Record<string, string>,
): Promise<{ valid: boolean; reason?: string }> {
  const factory = getDefaultWebhookFactory();
  return factory.validateWebhook(providerId, rawBody, signature, headers);
}

// Default export for convenience
export default {
  createWebhookSystem,
  getDefaultWebhookFactory,
  processWebhook,
  validateWebhook,
  // Core exports
  WebhookRegistryImpl,
  WebhookFactory,
  PaycrestWebhookProvider,
  TransactionUpdateHandler,
};