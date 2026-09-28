/**
 * Basic Webhook System Usage Examples
 * 
 * This file demonstrates common usage patterns for the modular webhook system.
 */

import { 
  createWebhookSystem,
  getDefaultWebhookFactory,
  processWebhook,
  validateWebhook,
  PaycrestWebhookProvider,
  TransactionUpdateHandler,
  WebhookRegistryImpl,
  WebhookFactory,
} from '@/lib/webhook';

/**
 * Example 1: Basic webhook processing
 */
async function exampleBasicProcessing() {
  console.log('=== Example 1: Basic Webhook Processing ===');
  
  // Create webhook system with default configuration
  const webhookSystem = createWebhookSystem();
  
  // Sample webhook data
  const rawBody = JSON.stringify({
    event: 'payment_order.settled',
    data: {
      id: 'order_1234567890',
      amount: '100.00',
      currency: 'NGN',
      status: 'settled',
    },
    timestamp: Date.now(),
  });
  
  const headers = {
    'x-paycrest-signature': 'valid-signature-here',
    'x-paycrest-timestamp': String(Date.now()),
    'x-paycrest-nonce': 'nonce-123456',
  };
  
  try {
    // Validate webhook signature
    const validation = await webhookSystem.validateWebhook(
      'paycrest',
      rawBody,
      headers['x-paycrest-signature'],
      headers
    );
    
    console.log('Validation result:', validation);
    
    if (validation.valid) {
      // Process webhook
      const results = await webhookSystem.processWebhook('paycrest', rawBody, headers);
      console.log('Processing results:', results);
    }
  } catch (error) {
    console.error('Error processing webhook:', error);
  }
}

/**
 * Example 2: Custom provider configuration
 */
async function exampleCustomConfiguration() {
  console.log('\n=== Example 2: Custom Configuration ===');
  
  // Create custom webhook system configuration
  const customSystem = createWebhookSystem({
    paycrest: {
      webhookSecret: 'your-custom-secret',
      timestampToleranceMs: 10 * 60 * 1000, // 10 minutes tolerance
      requireNonce: false, // Disable nonce validation for testing
    },
    // Disable default handlers if you want to register only specific ones
    registerDefaultHandlers: false,
  });
  
  // Manually register handlers
  const registry = customSystem.getRegistry();
  registry.registerHandler(
    TransactionUpdateHandler.createPaycrestHandler('payment_order.settled', 0)
  );
  registry.registerHandler(
    TransactionUpdateHandler.createPaycrestHandler('payment_order.refunded', 0)
  );
  
  console.log('Registered providers:', registry.getProviderIds());
  console.log('Registered event types:', registry.getEventTypes());
}

/**
 * Example 3: Creating a custom webhook provider
 */
async function exampleCustomProvider() {
  console.log('\n=== Example 3: Custom Provider ===');
  
  // Import necessary types
  const { BaseWebhookProvider } = await import('@/lib/webhook/core/base-provider');
  const { WebhookEvent, WebhookProcessResult } = await import('@/lib/webhook/core/interfaces');
  
  // Create a custom provider for a hypothetical service
  class CustomPaymentProvider extends BaseWebhookProvider {
    readonly providerId = 'custom-payment';
    readonly displayName = 'Custom Payment Service';
    
    async validateSignature(payload: string, signature: string, secret: string): Promise<boolean> {
      // Implement custom signature validation
      return this.validateHmacSignature(payload, signature, secret, 'sha512');
    }
    
    async handleEvent(event: WebhookEvent): Promise<WebhookProcessResult> {
      console.log(`Processing ${this.providerId} event:`, event.event);
      
      // Custom business logic here
      if (event.event === 'payment.completed') {
        return {
          success: true,
          metadata: {
            processed: true,
            amount: event.data.amount,
            currency: event.data.currency,
          },
        };
      }
      
      return {
        success: false,
        error: `Unhandled event type: ${event.event}`,
      };
    }
  }
  
  // Create system with custom provider
  const system = createWebhookSystem({
    providers: [
      new CustomPaymentProvider(),
    ],
  });
  
  // Process webhook with custom provider
  const customBody = JSON.stringify({
    event: 'payment.completed',
    data: {
      payment_id: 'pay_123',
      amount: 50.00,
      currency: 'USD',
    },
  });
  
  const customHeaders = {
    'x-custom-signature': 'custom-sig',
  };
  
  try {
    const results = await system.processWebhook('custom-payment', customBody, customHeaders);
    console.log('Custom provider results:', results);
  } catch (error) {
    console.error('Custom provider error:', error);
  }
}

/**
 * Example 4: Using utility functions
 */
async function exampleUtilityFunctions() {
  console.log('\n=== Example 4: Utility Functions ===');
  
  // Get default factory (configured from environment variables)
  const defaultFactory = getDefaultWebhookFactory();
  
  // Use standalone functions
  const rawBody = JSON.stringify({
    event: 'payment_order.pending',
    data: { id: 'order_789' },
  });
  
  const headers = {
    'x-paycrest-signature': 'test-signature',
    'x-paycrest-timestamp': String(Date.now()),
    'x-paycrest-nonce': 'nonce-789',
  };
  
  try {
    // Validate using standalone function
    const validation = await validateWebhook(
      'paycrest',
      rawBody,
      headers['x-paycrest-signature'],
      headers
    );
    console.log('Validation via utility:', validation);
    
    // Process using standalone function
    const results = await processWebhook('paycrest', rawBody, headers);
    console.log('Processing via utility:', results);
  } catch (error) {
    console.error('Utility function error:', error);
  }
}

/**
 * Example 5: Error handling patterns
 */
async function exampleErrorHandling() {
  console.log('\n=== Example 5: Error Handling ===');
  
  const { 
    SignatureValidationError,
    EventParsingError,
    ProviderNotFoundError,
  } = await import('@/lib/webhook/core/errors');
  
  const webhookSystem = createWebhookSystem();
  
  // Test with invalid provider
  try {
    await webhookSystem.processWebhook('invalid-provider', '{}', {});
  } catch (error) {
    if (error instanceof ProviderNotFoundError) {
      console.log('Caught ProviderNotFoundError:', error.message);
    }
  }
  
  // Test with invalid signature
  try {
    await webhookSystem.validateWebhook('paycrest', 'test', 'invalid-sig', {});
  } catch (error) {
    if (error instanceof SignatureValidationError) {
      console.log('Caught SignatureValidationError:', error.message);
    }
  }
  
  // Test with invalid JSON
  try {
    await webhookSystem.processWebhook('paycrest', 'invalid-json', {});
  } catch (error) {
    if (error instanceof EventParsingError) {
      console.log('Caught EventParsingError:', error.message);
    }
  }
}

/**
 * Main function to run all examples
 */
async function runExamples() {
  console.log('Running Webhook System Examples\n');
  
  await exampleBasicProcessing();
  await exampleCustomConfiguration();
  await exampleCustomProvider();
  await exampleUtilityFunctions();
  await exampleErrorHandling();
  
  console.log('\n=== All examples completed ===');
}

// Export for use in documentation
export {
  exampleBasicProcessing,
  exampleCustomConfiguration,
  exampleCustomProvider,
  exampleUtilityFunctions,
  exampleErrorHandling,
  runExamples,
};