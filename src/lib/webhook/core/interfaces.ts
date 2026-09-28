/**
 * Core Webhook Interfaces and Abstractions
 * 
 * This module defines the foundational interfaces for the modular webhook system.
 * These interfaces allow for different webhook providers (Paycrest, Stripe, etc.)
 * to be implemented consistently.
 */

export interface WebhookEvent {
  /** Unique event identifier */
  id?: string;
  /** Event type (e.g., 'payment_order.settled', 'payment_order.refunded') */
  event: string;
  /** Raw event data */
  data: Record<string, unknown>;
  /** When the event occurred (ISO string or Unix timestamp) */
  timestamp?: string | number;
  /** Optional metadata */
  metadata?: Record<string, unknown>;
}

export interface WebhookProvider {
  /** Unique provider identifier (e.g., 'paycrest', 'stripe') */
  providerId: string;
  
  /** Provider display name */
  displayName: string;
  
  /** Validate webhook signature from this provider */
  validateSignature(payload: string, signature: string, secret: string): Promise<boolean>;
  
  /** Parse raw request body into WebhookEvent */
  parseEvent(rawBody: string, headers: Record<string, string>): Promise<WebhookEvent>;
  
  /** Handle webhook event (process and apply business logic) */
  handleEvent(event: WebhookEvent): Promise<WebhookProcessResult>;
}

export interface WebhookHandler {
  /** Event type this handler can process (e.g., 'payment_order.settled') */
  eventType: string;
  
  /** Priority (lower numbers execute first) */
  priority?: number;
  
  /** Process the webhook event */
  process(event: WebhookEvent): Promise<WebhookProcessResult>;
}

export interface WebhookProcessResult {
  /** Whether processing was successful */
  success: boolean;
  
  /** Optional transaction ID affected by this webhook */
  transactionId?: string;
  
  /** Optional error message if processing failed */
  error?: string;
  
  /** Additional result metadata */
  metadata?: Record<string, unknown>;
}

export interface WebhookRegistry {
  /** Register a webhook provider */
  registerProvider(provider: WebhookProvider): void;
  
  /** Register a webhook handler for a specific event type */
  registerHandler(handler: WebhookHandler): void;
  
  /** Get provider by ID */
  getProvider(providerId: string): WebhookProvider | undefined;
  
  /** Get all handlers for an event type (sorted by priority) */
  getHandlers(eventType: string): WebhookHandler[];
  
  /** Process webhook for a specific provider */
  processWebhook(providerId: string, rawBody: string, headers: Record<string, string>): Promise<WebhookProcessResult[]>;
}