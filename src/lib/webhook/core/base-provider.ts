/**
 * Base Webhook Provider
 * 
 * Abstract implementation of WebhookProvider with common functionality.
 * Extend this class to implement specific webhook providers.
 */

import { WebhookProvider, WebhookEvent, WebhookProcessResult } from './interfaces';
import { SignatureValidationError, EventParsingError } from './errors';

export abstract class BaseWebhookProvider implements WebhookProvider {
  abstract providerId: string;
  abstract displayName: string;
  
  protected readonly defaultHeaders: Record<string, string> = {};
  
  /**
   * Validate webhook signature (to be implemented by concrete providers)
   */
  abstract validateSignature(payload: string, signature: string, secret: string): Promise<boolean>;
  
  /**
   * Parse raw request body into WebhookEvent
   * This can be overridden by concrete providers if needed
   */
  async parseEvent(rawBody: string, headers: Record<string, string>): Promise<WebhookEvent> {
    try {
      const parsed = JSON.parse(rawBody);
      
      if (!parsed.event) {
        throw new EventParsingError(this.providerId, 'Missing event type in webhook payload');
      }
      
      if (!parsed.data) {
        throw new EventParsingError(this.providerId, 'Missing data in webhook payload');
      }
      
      return {
        event: parsed.event,
        data: parsed.data,
        timestamp: parsed.timestamp || headers['x-payload-timestamp'] || Date.now(),
        id: parsed.id || headers['x-webhook-id'],
        metadata: {
          ...parsed.metadata,
          headers,
        },
      };
    } catch (error) {
      if (error instanceof EventParsingError) {
        throw error;
      }
      throw new EventParsingError(this.providerId, `Failed to parse webhook payload: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  
  /**
   * Handle webhook event by delegating to registered handlers
   * Concrete providers can override this for custom handling logic
   */
  abstract handleEvent(event: WebhookEvent): Promise<WebhookProcessResult>;
  
  /**
   * Common HMAC SHA256 signature validation
   * Can be used by providers that use HMAC signatures
   */
  protected async validateHmacSignature(
    payload: string,
    signature: string,
    secret: string,
    algorithm: string = 'sha256',
  ): Promise<boolean> {
    if (!signature) {
      throw new SignatureValidationError(this.providerId, 'Missing signature header');
    }
    
    if (!secret) {
      throw new SignatureValidationError(this.providerId, 'Missing webhook secret');
    }
    
    // Import crypto module dynamically for compatibility
    const crypto = await import('crypto');
    
    const expectedSignature = crypto
      .createHmac(algorithm, secret)
      .update(payload)
      .digest('hex');
    
    // Constant-time comparison to prevent timing attacks
    const signatureBuffer = Buffer.from(signature);
    const expectedBuffer = Buffer.from(expectedSignature);
    
    if (signatureBuffer.length !== expectedBuffer.length) {
      return false;
    }
    
    return crypto.timingSafeEqual(signatureBuffer, expectedBuffer);
  }
  
  /**
   * Validate webhook timestamp to prevent replay attacks
   */
  protected validateTimestamp(
    timestamp: string | number | undefined,
    toleranceMs: number = 5 * 60 * 1000, // 5 minutes tolerance
  ): boolean {
    if (!timestamp) {
      return false; // No timestamp provided
    }
    
    const timestampMs = typeof timestamp === 'string' ? parseInt(timestamp, 10) : timestamp;
    const now = Date.now();
    const diff = Math.abs(now - timestampMs);
    
    return diff <= toleranceMs;
  }
  
  /**
   * Validate nonce to prevent replay attacks
   * Implementations should store and check nonces
   */
  protected async validateNonce(
    nonce: string | undefined,
    store: { hasNonce: (nonce: string) => Promise<boolean>; addNonce: (nonce: string, ttl: number) => Promise<void> },
    ttlMs: number = 10 * 60 * 1000, // 10 minutes TTL
  ): Promise<boolean> {
    if (!nonce) {
      return false; // No nonce provided
    }
    
    const hasNonce = await store.hasNonce(nonce);
    if (hasNonce) {
      return false; // Nonce already used
    }
    
    await store.addNonce(nonce, ttlMs);
    return true;
  }
}