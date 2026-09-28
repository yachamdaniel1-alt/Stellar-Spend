import { NextRequest, NextResponse } from 'next/server';
import { checkRateLimit, InMemoryRateLimitStore } from '@/lib/rateLimiter';
import { logger } from '@/lib/logger';

// Global rate limit store instance
const rateLimitStore = new InMemoryRateLimitStore();

/**
 * Rate limiting middleware for API routes
 * Enforces per-endpoint rate limits with different thresholds for authenticated users
 */
export async function rateLimitMiddleware(
  request: NextRequest,
  endpoint: string,
  isAuthenticated: boolean = false,
): Promise<NextResponse | null> {
  try {
    // Get client identifier (IP address or user ID)
    const identifier = isAuthenticated
      ? request.headers.get('x-user-id') || getClientIp(request) || 'unknown'
      : getClientIp(request) || 'unknown';

    // Check rate limit
    const { allowed, remaining, resetTime } = await checkRateLimit(
      rateLimitStore,
      endpoint,
      identifier,
      isAuthenticated,
    );

    // Add rate limit headers to response
    const responseHeaders: Record<string, string> = {
      'X-RateLimit-Remaining': remaining.toString(),
      'X-RateLimit-Reset': Math.ceil(resetTime / 1000).toString(),
    };

    if (!allowed) {
      logger.warn('Rate limit exceeded', {
        endpoint,
        identifier,
        isAuthenticated,
      });

      return new NextResponse(
        JSON.stringify({
          error: 'Too many requests',
          message: 'Rate limit exceeded. Please try again later.',
          retryAfter: Math.ceil((resetTime - Date.now()) / 1000),
        }),
        {
          status: 429,
          headers: {
            ...responseHeaders,
            'Retry-After': Math.ceil((resetTime - Date.now()) / 1000).toString(),
            'Content-Type': 'application/json',
          },
        },
      );
    }

    // Request is allowed, return null to continue processing
    return null;
  } catch (error) {
    logger.error('Rate limiting middleware error', { error });
    // On error, allow the request to proceed
    return null;
  }
}

/**
 * Get client IP address from request
 */
function getClientIp(request: NextRequest): string | null {
  const forwarded = request.headers.get('x-forwarded-for');
  if (forwarded) {
    return forwarded.split(',')[0].trim();
  }
  return request.headers.get('x-real-ip');
}

/**
 * Reset rate limit for a specific endpoint and identifier
 * Useful for testing or administrative purposes
 */
export async function resetRateLimit(endpoint: string, identifier: string): Promise<void> {
  const key = `ratelimit:anon:${endpoint}:${identifier}`;
  await rateLimitStore.reset(key);
}

/**
 * Get current rate limit status for debugging
 */
export async function getRateLimitStatus(
  endpoint: string,
  identifier: string,
  isAuthenticated: boolean = false,
): Promise<{ current: number; limit: number | null }> {
  const key = `ratelimit:${isAuthenticated ? 'auth' : 'anon'}:${endpoint}:${identifier}`;
  const current = await rateLimitStore.get(key);

  // This is a simplified version - in production you'd want to get the actual limit
  return { current, limit: null };
}
