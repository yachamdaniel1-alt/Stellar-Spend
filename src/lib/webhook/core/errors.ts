/**
 * Webhook-specific error types
 */

export class WebhookError extends Error {
  constructor(
    message: string,
    public readonly code: string,
    public readonly providerId?: string,
    public readonly eventType?: string,
  ) {
    super(message);
    this.name = 'WebhookError';
  }
}

export class SignatureValidationError extends WebhookError {
  constructor(providerId: string, message: string = 'Invalid webhook signature') {
    super(message, 'SIGNATURE_VALIDATION_FAILED', providerId);
    this.name = 'SignatureValidationError';
  }
}

export class EventParsingError extends WebhookError {
  constructor(providerId: string, message: string = 'Failed to parse webhook event') {
    super(message, 'EVENT_PARSING_FAILED', providerId);
    this.name = 'EventParsingError';
  }
}

export class ProviderNotFoundError extends WebhookError {
  constructor(providerId: string) {
    super(`Webhook provider not found: ${providerId}`, 'PROVIDER_NOT_FOUND', providerId);
    this.name = 'ProviderNotFoundError';
  }
}

export class HandlerNotFoundError extends WebhookError {
  constructor(eventType: string) {
    super(`No handler found for event type: ${eventType}`, 'HANDLER_NOT_FOUND', undefined, eventType);
    this.name = 'HandlerNotFoundError';
  }
}

export class EventProcessingError extends WebhookError {
  constructor(
    eventType: string,
    message: string = 'Failed to process webhook event',
    providerId?: string,
  ) {
    super(message, 'EVENT_PROCESSING_FAILED', providerId, eventType);
    this.name = 'EventProcessingError';
  }
}