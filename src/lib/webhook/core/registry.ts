/**
 * Webhook Registry Implementation
 * 
 * Central registry for webhook providers and handlers.
 */

import { WebhookProvider, WebhookHandler, WebhookRegistry, WebhookProcessResult } from './interfaces';
import { ProviderNotFoundError, HandlerNotFoundError } from './errors';

export class WebhookRegistryImpl implements WebhookRegistry {
  private providers = new Map<string, WebhookProvider>();
  private handlers = new Map<string, WebhookHandler[]>();
  
  /**
   * Register a webhook provider
   */
  registerProvider(provider: WebhookProvider): void {
    if (this.providers.has(provider.providerId)) {
      throw new Error(`Provider '${provider.providerId}' is already registered`);
    }
    
    this.providers.set(provider.providerId, provider);
  }
  
  /**
   * Register a webhook handler for a specific event type
   */
  registerHandler(handler: WebhookHandler): void {
    const eventType = handler.eventType;
    
    if (!this.handlers.has(eventType)) {
      this.handlers.set(eventType, []);
    }
    
    const handlers = this.handlers.get(eventType)!;
    handlers.push(handler);
    
    // Sort by priority (lower numbers first)
    handlers.sort((a, b) => (a.priority || 0) - (b.priority || 0));
  }
  
  /**
   * Get provider by ID
   */
  getProvider(providerId: string): WebhookProvider | undefined {
    return this.providers.get(providerId);
  }
  
  /**
   * Get all handlers for an event type (sorted by priority)
   */
  getHandlers(eventType: string): WebhookHandler[] {
    return this.handlers.get(eventType) || [];
  }
  
  /**
   * Process webhook for a specific provider
   */
  async processWebhook(
    providerId: string,
    rawBody: string,
    headers: Record<string, string>,
  ): Promise<WebhookProcessResult[]> {
    const provider = this.getProvider(providerId);
    if (!provider) {
      throw new ProviderNotFoundError(providerId);
    }
    
    const event = await provider.parseEvent(rawBody, headers);
    const result = await provider.handleEvent(event);
    
    return [result];
  }
  
  /**
   * Process webhook event through all registered handlers for the event type
   * This can be used when providers want to delegate to the registry's handlers
   */
  async processEventThroughHandlers(event: { event: string }): Promise<WebhookProcessResult[]> {
    const eventType = event.event;
    const handlers = this.getHandlers(eventType);
    
    if (handlers.length === 0) {
      throw new HandlerNotFoundError(eventType);
    }
    
    const results: WebhookProcessResult[] = [];
    
    for (const handler of handlers) {
      try {
        const result = await handler.process(event as any);
        results.push(result);
      } catch (error) {
        results.push({
          success: false,
          error: error instanceof Error ? error.message : String(error),
          metadata: {
            handler: handler.eventType,
            error,
          },
        });
      }
    }
    
    return results;
  }
  
  /**
   * Get all registered provider IDs
   */
  getProviderIds(): string[] {
    return Array.from(this.providers.keys());
  }
  
  /**
   * Get all registered event types
   */
  getEventTypes(): string[] {
    return Array.from(this.handlers.keys());
  }
  
  /**
   * Check if a provider is registered
   */
  hasProvider(providerId: string): boolean {
    return this.providers.has(providerId);
  }
  
  /**
   * Check if there are handlers for an event type
   */
  hasHandlers(eventType: string): boolean {
    const handlers = this.handlers.get(eventType);
    return !!handlers && handlers.length > 0;
  }
}