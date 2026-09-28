/**
 * Centralized rate-limiter service — consolidated version
 *
 * This file combines functionality from:
 *   • src/lib/rate-limiting.ts (InMemoryRateLimitStore, endpoint-based configs)
 *   • src/lib/rateLimiter.ts (SlidingWindowRateLimiter, namespace-based configs)
 *
 * Provides a single authoritative place for all rate-limit configuration and
 * the shared `SlidingWindowRateLimiter` used across auth, transactions,
 * onramp, and offramp routes.
 *
 * Usage
 * ─────
 *   import { buildTxLimiter, paycrestOrderLimiter, applyRateLimit } from '@/lib/rateLimiter';
 *
 *   // Inside a Next.js route handler:
 *   const limitResult = await applyRateLimit(request, 'build-tx');
 *   if (limitResult) return limitResult; // 429 response
 *
 *   // Or using endpoint-based rate limiting:
 *   import { checkRateLimit, InMemoryRateLimitStore } from '@/lib/rateLimiter';
 *   const result = await checkRateLimit(store, endpoint, identifier, isAuthenticated);
 */

import type { NextRequest } from 'next/server';
import { NextResponse } from 'next/server';
import { logger } from './logger';
import { getCacheClient } from './cache/client';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface RateLimitConfig {
  /** Maximum number of requests allowed within the window. */
  maxRequests: number;
  /** Sliding window duration in milliseconds. */
  windowMs: number;
  /** If true, premium/authenticated users bypass this limiter. */
  premiumBypass?: boolean;
}

export interface RateLimitResult {
  allowed: boolean;
  limit: number;
  remaining: number;
  /** Unix timestamp (ms) when the window resets. */
  resetAt: number;
  /** Seconds until next request is allowed (only set when !allowed). */
  retryAfter?: number;
}

export interface RateLimitHeaders {
  'X-RateLimit-Limit': string;
  'X-RateLimit-Remaining': string;
  'X-RateLimit-Reset': string;
  'Retry-After'?: string;
}

// ── In-memory rate limit store (compatibility with old rate-limiting.ts) ──────

export interface RateLimitStore {
  get(key: string): Promise<number>;
  set(key: string, value: number, ttl: number): Promise<void>;
  increment(key: string, ttl: number): Promise<number>;
  reset(key: string): Promise<void>;
}

/**
 * In-memory rate limit store (suitable for single-instance deployments)
 * This provides compatibility with the old rate-limiting.ts module
 */
export class InMemoryRateLimitStore implements RateLimitStore {
  private store: Map<string, { count: number; resetTime: number }> = new Map();

  async get(key: string): Promise<number> {
    const entry = this.store.get(key);
    if (!entry) return 0;
    if (Date.now() > entry.resetTime) {
      this.store.delete(key);
      return 0;
    }
    return entry.count;
  }

  async set(key: string, value: number, ttl: number): Promise<void> {
    this.store.set(key, {
      count: value,
      resetTime: Date.now() + ttl,
    });
  }

  async increment(key: string, ttl: number): Promise<number> {
    const current = await this.get(key);
    const newCount = current + 1;
    await this.set(key, newCount, ttl);
    return newCount;
  }

  async reset(key: string): Promise<void> {
    this.store.delete(key);
  }
}

// ── Per-route rate-limit configuration ───────────────────────────────────────

/**
 * Central registry of rate-limit configs keyed by namespace.
 * Each entry maps directly to one SlidingWindowRateLimiter instance.
 */
export const RATE_LIMIT_REGISTRY: Record<string, RateLimitConfig> = {
  // ── Auth routes ───────────────────────────────────────────────────────────
  'auth-2fa': { maxRequests: 5, windowMs: 60_000 },

  // ── Bridge routes ─────────────────────────────────────────────────────────
  'build-tx': { maxRequests: 10, windowMs: 60_000, premiumBypass: false },
  'submit-soroban': { maxRequests: 5, windowMs: 60_000 },
  'bridge-status': { maxRequests: 60, windowMs: 60_000 },

  // ── Quote / rate routes ───────────────────────────────────────────────────
  quote: { maxRequests: 30, windowMs: 60_000, premiumBypass: true },
  'fx-rate': { maxRequests: 20, windowMs: 10_000 },

  // ── Payout / offramp routes ───────────────────────────────────────────────
  'paycrest-order': { maxRequests: 5, windowMs: 60_000, premiumBypass: true },
  'execute-payout': { maxRequests: 5, windowMs: 60_000 },
  'offramp-status': { maxRequests: 60, windowMs: 10_000 },

  // ── Transaction routes ────────────────────────────────────────────────────
  'transactions-read': { maxRequests: 60, windowMs: 60_000 },
  'transactions-write': { maxRequests: 20, windowMs: 60_000 },

  // ── Global fallback ───────────────────────────────────────────────────────
  global: { maxRequests: 100, windowMs: 60_000, premiumBypass: true },
};

// ── SlidingWindowRateLimiter ──────────────────────────────────────────────────

/**
 * Sliding-window rate limiter backed by the shared cache client.
 * Falls back gracefully to in-process storage when cache is unavailable.
 */
export class SlidingWindowRateLimiter {
  constructor(
    public readonly namespace: string,
    private readonly config: RateLimitConfig,
  ) {}

  /**
   * Record a request and return whether it is allowed.
   *
   * @param key       - Per-user/IP identifier (e.g. IP address or user ID)
   * @param options   - Optional flags (isPremium for bypass)
   */
  async check(key: string, options?: { isPremium?: boolean }): Promise<RateLimitResult> {
    if (options?.isPremium && this.config.premiumBypass) {
      return {
        allowed: true,
        limit: this.config.maxRequests,
        remaining: this.config.maxRequests,
        resetAt: Date.now() + this.config.windowMs,
      };
    }

    const storeKey = `rl:${this.namespace}:${key}`;
    const now = Date.now();
    const windowStart = now - this.config.windowMs;
    const resetAt = now + this.config.windowMs;

    const client = getCacheClient();
    const raw = await client.get(storeKey);
    let timestamps: number[] = raw ? (JSON.parse(raw) as number[]) : [];

    // Remove timestamps outside the current window
    timestamps = timestamps.filter((t) => t > windowStart);

    const count = timestamps.length;

    if (count >= this.config.maxRequests) {
      const oldestInWindow = timestamps[0];
      const retryAfterMs = oldestInWindow + this.config.windowMs - now;
      const retryAfter = Math.ceil(retryAfterMs / 1000);

      logger.warn('rate_limit.violation', {
        namespace: this.namespace,
        key,
        count,
        limit: this.config.maxRequests,
      });

      return {
        allowed: false,
        limit: this.config.maxRequests,
        remaining: 0,
        resetAt: oldestInWindow + this.config.windowMs,
        retryAfter,
      };
    }

    timestamps.push(now);
    const ttlSeconds = Math.ceil(this.config.windowMs / 1000);
    await client.set(storeKey, JSON.stringify(timestamps), ttlSeconds);

    return {
      allowed: true,
      limit: this.config.maxRequests,
      remaining: this.config.maxRequests - timestamps.length,
      resetAt,
    };
  }

  /** Reset the rate limit for a specific key (useful for tests / admin). */
  async reset(key: string): Promise<void> {
    await getCacheClient().del(`rl:${this.namespace}:${key}`);
  }
}

// ── Pre-configured limiter instances ─────────────────────────────────────────

function makeLimiter(namespace: string): SlidingWindowRateLimiter {
  const config = RATE_LIMIT_REGISTRY[namespace];
  if (!config) throw new Error(`No rate-limit config found for namespace "${namespace}"`);
  return new SlidingWindowRateLimiter(namespace, config);
}

/** Bridge build-transaction endpoint. */
export const buildTxLimiter = makeLimiter('build-tx');

/** Paycrest payout-order endpoint. */
export const paycrestOrderLimiter = makeLimiter('paycrest-order');

/** Quote / FX quote endpoint. */
export const quoteLimiter = makeLimiter('quote');

/** Global API limiter (fallback). */
export const globalApiLimiter = makeLimiter('global');

/** Offramp status polling. */
export const offrampStatusLimiter = makeLimiter('offramp-status');

/** Transaction read endpoints. */
export const transactionsReadLimiter = makeLimiter('transactions-read');

/** Transaction write endpoints. */
export const transactionsWriteLimiter = makeLimiter('transactions-write');

/** Auth / 2FA endpoints. */
export const authLimiter = makeLimiter('auth-2fa');

// ── Key extraction helpers ────────────────────────────────────────────────────

/** Extract the client IP address from a request. */
export function getClientIp(request: Request): string {
  const forwarded = request.headers.get('x-forwarded-for');
  if (forwarded) return forwarded.split(',')[0].trim();
  const realIp = request.headers.get('x-real-ip');
  if (realIp) return realIp;
  return 'unknown';
}

/** Return user identifier if present, otherwise fall back to IP. */
export function getRateLimitKey(request: Request): string {
  const auth = request.headers.get('authorization') ?? '';
  if (auth.startsWith('Bearer ')) return `user:${auth.slice(7, 15)}`;
  const apiKey = request.headers.get('x-api-key');
  if (apiKey) return `apikey:${apiKey.slice(0, 8)}`;
  return getClientIp(request);
}

// ── Rate-limit header helpers ────────────────────────────────────────────────

/** Build standard `X-RateLimit-*` headers from a RateLimitResult. */
export function getRateLimitHeaders(result: RateLimitResult): RateLimitHeaders {
  const headers: RateLimitHeaders = {
    'X-RateLimit-Limit': String(result.limit),
    'X-RateLimit-Remaining': String(result.remaining),
    'X-RateLimit-Reset': String(Math.ceil(result.resetAt / 1000)),
  };
  if (result.retryAfter !== undefined) {
    headers['Retry-After'] = String(result.retryAfter);
  }
  return headers;
}

// ── Shared middleware helper ──────────────────────────────────────────────────

/**
 * Apply rate limiting for a given namespace.
 *
 * Returns a `NextResponse` (429) when the limit is exceeded, or `null` when
 * the request should proceed.  Attach the returned response immediately:
 *
 * ```ts
 * const limited = await applyRateLimit(request, 'build-tx');
 * if (limited) return limited;
 * ```
 */
export async function applyRateLimit(
  request: NextRequest,
  namespace: string,
  options?: { isPremium?: boolean },
): Promise<NextResponse | null> {
  const config = RATE_LIMIT_REGISTRY[namespace];
  if (!config) {
    // No config registered → do not rate-limit
    return null;
  }

  const limiter = new SlidingWindowRateLimiter(namespace, config);
  const key = getRateLimitKey(request);
  const result = await limiter.check(key, options);

  const rlHeaders = getRateLimitHeaders(result);

  if (!result.allowed) {
    logger.warn('rate_limit.exceeded', { namespace, key });
    return new NextResponse(
      JSON.stringify({
        error: 'TOO_MANY_REQUESTS',
        message: 'Rate limit exceeded. Please try again later.',
        retryAfter: result.retryAfter,
      }),
      {
        status: 429,
        headers: {
          'Content-Type': 'application/json',
          ...rlHeaders,
        },
      },
    );
  }

  return null;
}


// ── Endpoint-based rate limiting (compatibility with old rate-limiting.ts) ───

/**
 * Rate limit configurations for different endpoints
 * This provides backward compatibility with the old rate-limiting.ts module
 */
export const RATE_LIMIT_CONFIGS: Record<string, { windowMs: number; maxRequests: number }> = {
  // Public endpoints - strict limits
  '/api/offramp/quote': {
    windowMs: 60 * 1000, // 1 minute
    maxRequests: 30,
  },
  '/api/offramp/currencies': {
    windowMs: 60 * 1000,
    maxRequests: 100,
  },
  '/api/offramp/rate': {
    windowMs: 10 * 1000, // 10 seconds
    maxRequests: 10,
  },

  // Bridge endpoints - moderate limits
  '/api/offramp/bridge/build-tx': {
    windowMs: 60 * 1000,
    maxRequests: 20,
  },
  '/api/offramp/bridge/submit-soroban': {
    windowMs: 60 * 1000,
    maxRequests: 10,
  },
  '/api/offramp/bridge/status': {
    windowMs: 10 * 1000,
    maxRequests: 30,
  },

  // Payout endpoints - strict limits
  '/api/offramp/paycrest/order': {
    windowMs: 60 * 1000,
    maxRequests: 5,
  },
  '/api/offramp/execute-payout': {
    windowMs: 60 * 1000,
    maxRequests: 5,
  },
  '/api/offramp/status': {
    windowMs: 10 * 1000,
    maxRequests: 30,
  },

  // Webhook endpoints - no rate limit (use signature verification instead)
  '/api/webhooks/paycrest': {
    windowMs: 60 * 1000,
    maxRequests: 1000, // Very high limit for webhooks
  },

  // Health check - no rate limit
  '/api/health': {
    windowMs: 60 * 1000,
    maxRequests: 1000,
  },
};

/**
 * Get rate limit config for an endpoint
 */
export function getRateLimitConfig(endpoint: string): { windowMs: number; maxRequests: number } | null {
  // Exact match
  if (RATE_LIMIT_CONFIGS[endpoint]) {
    return RATE_LIMIT_CONFIGS[endpoint];
  }

  // Pattern match (e.g., /api/offramp/bridge/status/[hash])
  for (const [pattern, config] of Object.entries(RATE_LIMIT_CONFIGS)) {
    const regex = new RegExp(`^${pattern.replace(/\[.*?\]/g, '[^/]+')}$`);
    if (regex.test(endpoint)) {
      return config;
    }
  }

  return null;
}

/**
 * Generate rate limit key from request
 */
export function generateRateLimitKey(
  endpoint: string,
  identifier: string,
  isAuthenticated: boolean,
): string {
  const prefix = isAuthenticated ? 'auth' : 'anon';
  return `ratelimit:${prefix}:${endpoint}:${identifier}`;
}

/**
 * Check if request should be rate limited
 * This provides compatibility with the old rate-limiting.ts module
 */
export async function checkRateLimit(
  store: RateLimitStore,
  endpoint: string,
  identifier: string,
  isAuthenticated: boolean,
): Promise<{ allowed: boolean; remaining: number; resetTime: number }> {
  const config = getRateLimitConfig(endpoint);
  if (!config) {
    return { allowed: true, remaining: -1, resetTime: 0 };
  }

  const key = generateRateLimitKey(endpoint, identifier, isAuthenticated);
  const current = await store.increment(key, config.windowMs);

  const allowed = current <= config.maxRequests;
  const remaining = Math.max(0, config.maxRequests - current);
  const resetTime = Date.now() + config.windowMs;

  return { allowed, remaining, resetTime };
}