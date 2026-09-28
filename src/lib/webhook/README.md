# Modular Webhook System

A modular, extensible webhook system for handling external service notifications. This system provides a clean separation between webhook providers, handlers, and core infrastructure.

## Architecture Overview

The webhook system is organized into several key components:

```
src/lib/webhook/
├── core/           # Core abstractions and interfaces
│   ├── interfaces.ts   # WebhookProvider, WebhookHandler, WebhookRegistry interfaces
│   ├── errors.ts       # Webhook-specific error types
│   ├── base-provider.ts # Abstract base provider implementation
│   └── registry.ts     # Webhook registry implementation
├── providers/      # Provider-specific implementations
│   └── paycrest-provider.ts # Paycrest webhook provider
├── handlers/       # Webhook event handlers
│   ├── base-handler.ts      # Abstract base handler
│   └── transaction-update-handler.ts # Transaction update handler
├── factory.ts      # Factory for creating webhook systems
├── index.ts        # Main entry point and exports
└── README.md       # This documentation
```

## Core Concepts

### WebhookProvider
Responsible for:
- Validating webhook signatures
- Parsing raw webhook data into standardized events
- Handling webhook events (or delegating to handlers)

### WebhookHandler
Responsible for:
- Processing specific event types
- Applying business logic to webhook events
- Returning processing results

### WebhookRegistry
Central registry that:
- Manages registered providers and handlers
- Routes webhooks to appropriate providers
- Coordinates event processing through handlers

## Usage Examples

### Basic Usage

```typescript
import { createWebhookSystem } from '@/lib/webhook';

// Create webhook system with default configuration
const webhookSystem = createWebhookSystem({
  paycrest: {
    webhookSecret: process.env.PAYCREST_WEBHOOK_SECRET,
    timestampToleranceMs: 5 * 60 * 1000, // 5 minutes
    requireNonce: true,
  },
});

// Process a webhook
const results = await webhookSystem.processWebhook(
  'paycrest',
  rawBody,
  headers
);

// Validate a webhook
const validation = await webhookSystem.validateWebhook(
  'paycrest',
  rawBody,
  signature,
  headers
);
```

### Creating a Custom Provider

```typescript
import { BaseWebhookProvider } from '@/lib/webhook/core/base-provider';
import { WebhookEvent, WebhookProcessResult } from '@/lib/webhook/core/interfaces';

export class StripeWebhookProvider extends BaseWebhookProvider {
  readonly providerId = 'stripe';
  readonly displayName = 'Stripe';

  async validateSignature(payload: string, signature: string, secret: string): Promise<boolean> {
    // Implement Stripe-specific signature validation
    return this.validateHmacSignature(payload, signature, secret, 'sha256');
  }

  async handleEvent(event: WebhookEvent): Promise<WebhookProcessResult> {
    // Implement Stripe event handling
    return {
      success: true,
      metadata: { event: event.event },
    };
  }
}
```

### Creating a Custom Handler

```typescript
import { BaseWebhookHandler } from '@/lib/webhook/handlers/base-handler';
import { WebhookEvent, WebhookProcessResult } from '@/lib/webhook/core/interfaces';

export class EmailNotificationHandler extends BaseWebhookHandler {
  readonly eventType = 'user.created';
  readonly priority = 10;

  async process(event: WebhookEvent): Promise<WebhookProcessResult> {
    try {
      const userData = event.data as { email: string; name: string };
      await sendWelcomeEmail(userData.email, userData.name);
      
      return {
        success: true,
        metadata: { emailSent: true },
      };
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }
}
```

### Advanced Configuration

```typescript
import { 
  createWebhookSystem, 
  PaycrestWebhookProvider,
  TransactionUpdateHandler 
} from '@/lib/webhook';

// Create a custom webhook system
const customSystem = createWebhookSystem({
  // Configure providers
  providers: [
    new PaycrestWebhookProvider({
      webhookSecret: process.env.PAYCREST_WEBHOOK_SECRET,
      timestampToleranceMs: 10 * 60 * 1000, // 10 minutes
      requireNonce: false,
    }),
  ],
  
  // Configure custom handlers
  handlers: [
    TransactionUpdateHandler.createPaycrestHandler('payment_order.settled', 0),
    TransactionUpdateHandler.createPaycrestHandler('payment_order.refunded', 0),
  ],
  
  // Disable default handlers if needed
  registerDefaultHandlers: false,
});

// Register additional handlers dynamically
customSystem.getRegistry().registerHandler(
  new EmailNotificationHandler()
);
```

## API Routes Integration

### Paycrest Webhook Route Example

```typescript
import { createWebhookSystem } from '@/lib/webhook';

export async function POST(request: NextRequest) {
  const rawBody = await request.text();
  const signature = request.headers.get('X-Paycrest-Signature') ?? '';
  
  const webhookSystem = createWebhookSystem();
  
  // Validate webhook
  const validation = await webhookSystem.validateWebhook(
    'paycrest',
    rawBody,
    signature,
    Object.fromEntries(request.headers.entries())
  );
  
  if (!validation.valid) {
    return NextResponse.json(
      { error: validation.reason },
      { status: 401 }
    );
  }
  
  // Process webhook
  const results = await webhookSystem.processWebhook(
    'paycrest',
    rawBody,
    Object.fromEntries(request.headers.entries())
  );
  
  return NextResponse.json({ results });
}
```

## Testing

### Unit Testing Providers

```typescript
import { PaycrestWebhookProvider } from '@/lib/webhook/providers/paycrest-provider';

describe('PaycrestWebhookProvider', () => {
  let provider: PaycrestWebhookProvider;
  
  beforeEach(() => {
    provider = new PaycrestWebhookProvider({
      webhookSecret: 'test-secret',
    });
  });
  
  it('should validate correct signatures', async () => {
    const payload = '{"event":"test","data":{}}';
    const signature = 'valid-signature';
    
    // Mock signature validation
    vi.spyOn(provider, 'validateSignature').mockResolvedValue(true);
    
    const valid = await provider.validateSignature(payload, signature, 'test-secret');
    expect(valid).toBe(true);
  });
});
```

### Integration Testing

```typescript
import { createWebhookSystem } from '@/lib/webhook';

describe('Webhook System Integration', () => {
  let webhookSystem: ReturnType<typeof createWebhookSystem>;
  
  beforeEach(() => {
    webhookSystem = createWebhookSystem({
      paycrest: {
        webhookSecret: 'test-secret',
      },
    });
  });
  
  it('should process paycrest webhooks', async () => {
    const rawBody = JSON.stringify({
      event: 'payment_order.settled',
      data: { id: 'order-123' },
    });
    
    const results = await webhookSystem.processWebhook('paycrest', rawBody, {});
    expect(results).toBeDefined();
  });
});
```

## Error Handling

The system provides specific error types for different failure scenarios:

```typescript
import { 
  SignatureValidationError,
  EventParsingError,
  ProviderNotFoundError,
  HandlerNotFoundError 
} from '@/lib/webhook/core/errors';

try {
  await webhookSystem.processWebhook('unknown', rawBody, headers);
} catch (error) {
  if (error instanceof ProviderNotFoundError) {
    console.error(`Provider not found: ${error.providerId}`);
  } else if (error instanceof SignatureValidationError) {
    console.error(`Invalid signature from ${error.providerId}`);
  }
}
```

## Configuration

### Environment Variables

```bash
# Paycrest webhook secret
PAYCREST_WEBHOOK_SECRET=your-secret-here

# Optional: Custom timestamp tolerance (milliseconds)
WEBHOOK_TIMESTAMP_TOLERANCE_MS=300000  # 5 minutes
```

### Webhook System Configuration Options

```typescript
interface WebhookSystemConfig {
  // Paycrest configuration
  paycrest?: {
    webhookSecret: string;
    timestampToleranceMs?: number;  // Default: 300000 (5 minutes)
    requireNonce?: boolean;         // Default: true
  };
  
  // Additional providers
  providers?: WebhookProvider[];
  
  // Additional handlers
  handlers?: WebhookHandler[];
  
  // Whether to register default handlers
  registerDefaultHandlers?: boolean; // Default: true
}
```

## Migration from Legacy System

The modular system maintains backward compatibility with the legacy `WebhookService`:

```typescript
// Legacy usage (still works)
import { WebhookService } from '@/lib/services/webhook.service';

const legacyService = new WebhookService();
const result = await legacyService.processPaycrestWebhook({
  event: 'payment_order.settled',
  data: { id: 'order-123' },
});

// New modular usage (recommended)
import { createWebhookSystem } from '@/lib/webhook';

const webhookSystem = createWebhookSystem();
const results = await webhookSystem.processWebhook(
  'paycrest',
  JSON.stringify({ event: 'payment_order.settled', data: { id: 'order-123' } }),
  {}
);
```

## Best Practices

1. **Always validate webhook signatures** before processing
2. **Use timestamp and nonce validation** to prevent replay attacks
3. **Handle webhook processing asynchronously** for better performance
4. **Log webhook processing results** for debugging and auditing
5. **Implement retry logic** for transient failures
6. **Monitor webhook delivery rates** and error rates

## Extension Points

The system is designed to be extended:

1. **Add new providers** by implementing the `WebhookProvider` interface
2. **Add new handlers** by implementing the `WebhookHandler` interface
3. **Customize event parsing** by extending `BaseWebhookProvider`
4. **Implement custom validation** by overriding provider methods
5. **Add middleware** by extending the registry or factory

## Performance Considerations

- **Signature validation** is computationally expensive; cache results when appropriate
- **Event parsing** should be lightweight; defer heavy processing to handlers
- **Handler execution** can be parallelized for independent events
- **Nonce storage** should use a distributed store (Redis) in production

## Security Considerations

1. **Never log sensitive data** from webhook payloads
2. **Use constant-time comparison** for signature validation
3. **Validate timestamps** to prevent replay attacks
4. **Implement rate limiting** for webhook endpoints
5. **Use secure storage** for webhook secrets
6. **Monitor for suspicious patterns** in webhook traffic