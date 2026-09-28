/**
 * API Route Integration Example
 * 
 * This file demonstrates how to integrate the webhook system with Next.js API routes.
 */

import { NextRequest, NextResponse } from 'next/server';
import { createWebhookSystem, validateWebhook, processWebhook } from '@/lib/webhook';
import { logger } from '@/lib/logger';

/**
 * Example 1: Simple webhook endpoint for Paycrest
 */
export async function POST_paycrest_webhook(request: NextRequest) {
  try {
    // Read raw request body
    const rawBody = await request.text();
    
    // Extract required headers
    const signature = request.headers.get('X-Paycrest-Signature') ?? '';
    const timestamp = request.headers.get('X-Paycrest-Timestamp') ?? '';
    const nonce = request.headers.get('X-Paycrest-Nonce') ?? '';
    
    // Prepare headers object
    const headers = {
      'x-paycrest-signature': signature,
      'x-paycrest-timestamp': timestamp,
      'x-paycrest-nonce': nonce,
      ...Object.fromEntries(request.headers.entries()),
    };
    
    // Validate webhook using the modular system
    const validation = await validateWebhook('paycrest', rawBody, signature, headers);
    
    if (!validation.valid) {
      logger.warn('webhook.validation_failed', {
        provider: 'paycrest',
        reason: validation.reason,
      });
      
      return NextResponse.json(
        { error: 'Invalid webhook signature', reason: validation.reason },
        { status: 401 }
      );
    }
    
    // Process webhook asynchronously
    processWebhook('paycrest', rawBody, headers)
      .then(results => {
        const successful = results.filter(r => r.success);
        const failed = results.filter(r => !r.success);
        
        logger.info('webhook.async_processing_complete', {
          provider: 'paycrest',
          successful: successful.length,
          failed: failed.length,
        });
      })
      .catch(error => {
        logger.error('webhook.async_processing_error', {
          provider: 'paycrest',
          error: error instanceof Error ? error.message : String(error),
        });
      });
    
    // Return 202 Accepted immediately
    return NextResponse.json(
      { received: true, message: 'Webhook accepted for processing' },
      { status: 202 }
    );
  } catch (error) {
    logger.error('webhook.endpoint_error', {
      error: error instanceof Error ? error.message : String(error),
    });
    
    return NextResponse.json(
      { error: 'Internal server error' },
      { status: 500 }
    );
  }
}

/**
 * Example 2: Generic webhook endpoint that supports multiple providers
 */
export async function POST_generic_webhook(
  request: NextRequest,
  { params }: { params: { provider: string } }
) {
  try {
    const provider = params.provider; // e.g., 'paycrest', 'stripe', 'custom'
    const rawBody = await request.text();
    
    // Get all headers
    const headers = Object.fromEntries(request.headers.entries());
    
    // Get signature header based on provider
    const signature = getSignatureForProvider(provider, headers);
    
    if (!signature) {
      return NextResponse.json(
        { error: `Missing signature for provider: ${provider}` },
        { status: 400 }
      );
    }
    
    // Create webhook system with provider-specific configuration
    const webhookSystem = createWebhookSystem();
    
    // Check if provider is registered
    const registry = webhookSystem.getRegistry();
    if (!registry.hasProvider(provider)) {
      return NextResponse.json(
        { error: `Unsupported webhook provider: ${provider}` },
        { status: 400 }
      );
    }
    
    // Validate webhook
    const validation = await webhookSystem.validateWebhook(
      provider,
      rawBody,
      signature,
      headers
    );
    
    if (!validation.valid) {
      logger.warn('webhook.validation_failed', {
        provider,
        reason: validation.reason,
      });
      
      return NextResponse.json(
        { error: 'Invalid webhook signature', reason: validation.reason },
        { status: 401 }
      );
    }
    
    // Process webhook synchronously (for immediate feedback)
    const results = await webhookSystem.processWebhook(provider, rawBody, headers);
    
    const successfulResults = results.filter(r => r.success);
    const failedResults = results.filter(r => !r.success);
    
    logger.info('webhook.processing_complete', {
      provider,
      successful: successfulResults.length,
      failed: failedResults.length,
    });
    
    // Return results
    return NextResponse.json({
      success: true,
      results: {
        total: results.length,
        successful: successfulResults.length,
        failed: failedResults.length,
        details: results.map(r => ({
          success: r.success,
          transactionId: r.transactionId,
          error: r.error,
        })),
      },
    });
  } catch (error) {
    logger.error('webhook.generic_endpoint_error', {
      provider: params.provider,
      error: error instanceof Error ? error.message : String(error),
    });
    
    return NextResponse.json(
      { error: 'Internal server error' },
      { status: 500 }
    );
  }
}

/**
 * Example 3: Webhook endpoint with idempotency and rate limiting
 */
export async function POST_secure_webhook(
  request: NextRequest,
  { params }: { params: { provider: string } }
) {
  try {
    const provider = params.provider;
    const rawBody = await request.text();
    const requestId = request.headers.get('X-Request-ID') || crypto.randomUUID();
    
    // Log request
    logger.info('webhook.request_received', {
      requestId,
      provider,
      contentLength: rawBody.length,
    });
    
    // Parse JSON early to validate structure
    let payload;
    try {
      payload = JSON.parse(rawBody);
    } catch {
      return NextResponse.json(
        { error: 'Invalid JSON payload' },
        { status: 400 }
      );
    }
    
    // Validate required fields
    if (!payload.event || !payload.data) {
      return NextResponse.json(
        { error: 'Missing required fields: event and data' },
        { status: 400 }
      );
    }
    
    // Extract headers
    const headers = {
      'x-request-id': requestId,
      ...Object.fromEntries(request.headers.entries()),
    };
    
    // Get signature
    const signature = getSignatureForProvider(provider, headers);
    if (!signature) {
      return NextResponse.json(
        { error: 'Missing signature header' },
        { status: 400 }
      );
    }
    
    // Create webhook system
    const webhookSystem = createWebhookSystem();
    
    // Validate
    const validation = await webhookSystem.validateWebhook(
      provider,
      rawBody,
      signature,
      headers
    );
    
    if (!validation.valid) {
      logger.warn('webhook.validation_failed', {
        requestId,
        provider,
        reason: validation.reason,
      });
      
      return NextResponse.json(
        { 
          error: 'Webhook validation failed',
          requestId,
          reason: validation.reason,
        },
        { status: 401 }
      );
    }
    
    // Check idempotency key (if provided)
    const idempotencyKey = request.headers.get('X-Idempotency-Key');
    if (idempotencyKey) {
      // Check if this request was already processed
      const isDuplicate = await checkIdempotency(idempotencyKey, provider, payload);
      if (isDuplicate) {
        logger.info('webhook.idempotent_duplicate', {
          requestId,
          provider,
          idempotencyKey,
        });
        
        return NextResponse.json(
          { 
            success: true,
            message: 'Request already processed',
            requestId,
            idempotencyKey,
          },
          { status: 200 }
        );
      }
    }
    
    // Process webhook
    const results = await webhookSystem.processWebhook(provider, rawBody, headers);
    
    // Store idempotency key if provided
    if (idempotencyKey) {
      await storeIdempotency(idempotencyKey, provider, payload, results);
    }
    
    // Prepare response
    const response = {
      requestId,
      provider,
      timestamp: new Date().toISOString(),
      results: results.map(r => ({
        success: r.success,
        transactionId: r.transactionId,
        error: r.error,
      })),
    };
    
    logger.info('webhook.processing_complete', {
      requestId,
      provider,
      successful: results.filter(r => r.success).length,
      total: results.length,
    });
    
    return NextResponse.json(response, { status: 200 });
  } catch (error) {
    logger.error('webhook.secure_endpoint_error', {
      provider: params.provider,
      error: error instanceof Error ? error.message : String(error),
    });
    
    return NextResponse.json(
      { error: 'Internal server error' },
      { status: 500 }
    );
  }
}

/**
 * Helper function to extract signature based on provider
 */
function getSignatureForProvider(provider: string, headers: Record<string, string>): string {
  switch (provider) {
    case 'paycrest':
      return headers['x-paycrest-signature'] || '';
    case 'stripe':
      return headers['stripe-signature'] || '';
    case 'github':
      return headers['x-hub-signature-256'] || '';
    default:
      return headers['x-webhook-signature'] || '';
  }
}

/**
 * Helper function to check idempotency (mock implementation)
 */
async function checkIdempotency(
  key: string,
  provider: string,
  payload: any
): Promise<boolean> {
  // In production, check a distributed cache like Redis
  // This is a simplified example
  return false;
}

/**
 * Helper function to store idempotency key (mock implementation)
 */
async function storeIdempotency(
  key: string,
  provider: string,
  payload: any,
  results: any[]
): Promise<void> {
  // In production, store in a distributed cache with TTL
  // This is a simplified example
}

/**
 * Example route handler configuration for Next.js
 */
export const maxDuration = 30; // seconds
export const dynamic = 'force-dynamic';

// Export for use in route files
export default {
  POST_paycrest_webhook,
  POST_generic_webhook,
  POST_secure_webhook,
};