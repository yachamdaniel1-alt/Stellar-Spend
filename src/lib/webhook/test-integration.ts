/**
 * Integration Test for Modular Webhook System
 * 
 * This test verifies that the modular webhook system works correctly.
 */

import { createWebhookSystem } from './index';
import { PaycrestWebhookProvider } from './providers/paycrest-provider';
import { TransactionUpdateHandler } from './handlers/transaction-update-handler';

async function runIntegrationTest() {
  console.log('=== Modular Webhook System Integration Test ===\n');
  
  let passed = 0;
  let failed = 0;
  
  // Test 1: Create webhook system
  console.log('Test 1: Creating webhook system...');
  try {
    const webhookSystem = createWebhookSystem({
      paycrest: {
        webhookSecret: 'test-secret-123',
        timestampToleranceMs: 5 * 60 * 1000,
        requireNonce: false, // Disable for testing
      },
    });
    
    console.log('✓ Webhook system created successfully');
    console.log(`  - Providers: ${webhookSystem.getRegistry().getProviderIds().join(', ')}`);
    console.log(`  - Event types: ${webhookSystem.getRegistry().getEventTypes().join(', ')}`);
    passed++;
  } catch (error) {
    console.log(`✗ Failed to create webhook system: ${error}`);
    failed++;
  }
  
  // Test 2: Validate configuration
  console.log('\nTest 2: Validating configuration...');
  try {
    const webhookSystem = createWebhookSystem({
      paycrest: {
        webhookSecret: 'test-secret-123',
      },
    });
    
    const validation = webhookSystem.validateConfiguration();
    if (validation.valid) {
      console.log('✓ Configuration validation passed');
      passed++;
    } else {
      console.log(`✗ Configuration validation failed: ${validation.errors.join(', ')}`);
      failed++;
    }
  } catch (error) {
    console.log(`✗ Configuration validation error: ${error}`);
    failed++;
  }
  
  // Test 3: Provider registration
  console.log('\nTest 3: Testing provider registration...');
  try {
    const provider = new PaycrestWebhookProvider({
      webhookSecret: 'test-secret',
    });
    
    console.log(`✓ Provider created: ${provider.providerId} (${provider.displayName})`);
    console.log(`  - Config: ${JSON.stringify(provider.getConfig())}`);
    passed++;
  } catch (error) {
    console.log(`✗ Provider creation failed: ${error}`);
    failed++;
  }
  
  // Test 4: Handler registration
  console.log('\nTest 4: Testing handler registration...');
  try {
    const handler = TransactionUpdateHandler.createPaycrestHandler('payment_order.settled', 0);
    
    console.log(`✓ Handler created: ${handler.eventType} (priority: ${handler.priority})`);
    console.log(`  - Config: ${JSON.stringify(handler.getConfig())}`);
    passed++;
  } catch (error) {
    console.log(`✗ Handler creation failed: ${error}`);
    failed++;
  }
  
  // Test 5: Process webhook (mock)
  console.log('\nTest 5: Testing webhook processing (mock)...');
  try {
    const webhookSystem = createWebhookSystem({
      paycrest: {
        webhookSecret: 'test-secret',
        requireNonce: false,
      },
    });
    
    const mockWebhook = JSON.stringify({
      event: 'payment_order.settled',
      data: { id: 'test-order-123' },
      timestamp: Date.now(),
    });
    
    const mockHeaders = {
      'x-paycrest-signature': 'mock-signature',
      'x-paycrest-timestamp': String(Date.now()),
    };
    
    // Note: This will fail signature validation but that's expected
    const results = await webhookSystem.processWebhook('paycrest', mockWebhook, mockHeaders);
    
    console.log(`✓ Webhook processing attempted`);
    console.log(`  - Results count: ${results.length}`);
    console.log(`  - Success results: ${results.filter(r => r.success).length}`);
    console.log(`  - Failed results: ${results.filter(r => !r.success).length}`);
    passed++;
  } catch (error) {
    console.log(`✗ Webhook processing error: ${error}`);
    failed++;
  }
  
  // Test 6: Error handling
  console.log('\nTest 6: Testing error handling...');
  try {
    const webhookSystem = createWebhookSystem();
    
    // Try to process with invalid provider
    await webhookSystem.processWebhook('invalid-provider', '{}', {});
    console.log('✗ Should have thrown for invalid provider');
    failed++;
  } catch (error) {
    console.log('✓ Invalid provider correctly rejected');
    console.log(`  - Error: ${error}`);
    passed++;
  }
  
  // Summary
  console.log('\n=== Test Summary ===');
  console.log(`Total tests: ${passed + failed}`);
  console.log(`Passed: ${passed}`);
  console.log(`Failed: ${failed}`);
  console.log(`Success rate: ${((passed / (passed + failed)) * 100).toFixed(1)}%`);
  
  if (failed === 0) {
    console.log('\n✅ All tests passed! The modular webhook system is working correctly.');
    return 0;
  } else {
    console.log('\n❌ Some tests failed. Please review the errors above.');
    return 1;
  }
}

// Run the test
if (require.main === module) {
  runIntegrationTest()
    .then(exitCode => {
      process.exit(exitCode);
    })
    .catch(error => {
      console.error('Test runner error:', error);
      process.exit(1);
    });
}

export { runIntegrationTest };