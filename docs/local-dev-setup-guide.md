# Local Development Setup Guide

> Unified reference for setting up and running the full Stellar-Spend stack locally — covering the **Node.js/Rust toolchain**, **Docker Compose infrastructure**, **Soroban smart contract build/test**, and **development server workflows**.

---

## Table of Contents

1. [Toolchain Requirements](#toolchain-requirements)
2. [Docker Compose Infrastructure](#docker-compose-infrastructure)
   - [docker-compose.yml](#docker-composeyml-primary-stack)
   - [docker-compose.canary.yml](#docker-composecanaryyml-canary-deployments)
3. [Contract Build & Test Commands](#contract-build--test-commands)
4. [Running the Development Stack](#running-the-development-stack)
   - [npm run dev](#npm-run-dev)
   - [docker-compose up](#docker-compose-up)
   - [cargo test](#cargo-test)
5. [Complete Setup Walkthrough](#complete-setup-walkthrough)
6. [Troubleshooting](#troubleshooting)

---

## Toolchain Requirements

| Component | Required Version | Purpose | Installation |
|-----------|-----------------|---------|--------------|
| **Node.js** | `20.19.2` (pinned in Dockerfile) | Next.js frontend, API routes, tooling | [nodejs.org](https://nodejs.org) or `nvm install 20.19.2` |
| **npm** | Bundled with Node.js | Package management | Bundled with Node.js |
| **TypeScript** | `5.7.3` (hard-pinned in `package.json`) | Type safety across the project | Bundled via `npm install` |
| **Next.js** | `^15.5.14` | App framework (App Router, API routes) | Bundled via `npm install` |
| **Rust** | Stable toolchain, edition `2021` | Compiling Soroban smart contracts | [rustup.rs](https://rustup.rs) |
| **wasm32 Target** | `wasm32-unknown-unknown` | Compiling Rust contracts to WebAssembly | `rustup target add wasm32-unknown-unknown` |
| **cargo-audit** | Latest | Security vulnerability scanning for Rust deps | `cargo install cargo-audit` |
| **stellar-cli** | `>= 21.0.0` | Contract building, deployment, simulation | `cargo install --locked stellar-cli --features opt` |
| **PostgreSQL** | `16-alpine` (Docker image) | Transaction records, idempotency keys, API keys | Docker Compose or local install |
| **Redis** | `7-alpine` (Docker image) | Caching, session store | Docker Compose or local install |

> **Note:** The `package.json` hard-pins `typescript` to `5.7.3` and `react`/`react-dom` to `19.0.0` — these are build contracts. The `Dockerfile` pins `node:20.19.2-alpine3.21` to ensure reproducible builds.

### Quick toolchain verification

```bash
node -v          # Must output v20.19.2 or compatible
npm -v           # Must be >= 10.x
rustc --version  # Must be a stable release supporting edition 2021
cargo --version
rustup target list --installed  # Verify wasm32-unknown-unknown
```

---

## Docker Compose Infrastructure

### docker-compose.yml — Primary Stack

The `docker-compose.yml` defines the **production-equivalent local infrastructure** with four services:

| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| `app` | `stellar-spend:latest` | `3000:3000` | Next.js application (built via `Dockerfile`) |
| `postgres` | `postgres:16-alpine` | `5432:5432` | Primary database |
| `redis` | `redis:7-alpine` | `6379:6379` | Cache and session store |
| `migrate` | `stellar-spend:latest` (builder target) | — | Runs `npx ts-node scripts/migrate.ts` (requires `--profile migrate`) |

**Key characteristics:**

- All services share the `stellar-net` bridge network
- Health checks gate startup: `postgres` waits for `pg_isready`, `redis` waits for `ping`, `app` waits for `/api/health`
- Persistent volumes: `postgres-data` and `redis-data`
- Resource limits: app `512M` / `256M`, postgres `256M` / `128M`, redis `128M` / `64M`
- Environment sourced from `.env.local` with `DATABASE_URL` and `REDIS_URL` wired explicitly
- The `migrate` service uses the `builder` build target and is behind the `migrate` profile — run with `docker compose --profile migrate up migrate`

**Start the primary stack:**

```bash
docker compose up -d
```

**Start with migrations:**

```bash
docker compose --profile migrate up -d migrate
```

**Stop everything:**

```bash
docker compose down
```

### docker-compose.canary.yml — Canary Deployments

The `docker-compose.canary.yml` is an **override file** that runs a second `app` container against the same Postgres/Redis infrastructure, enabling canary testing of new application code without affecting the primary deployment.

**Key differences from the primary stack:**

| Aspect | `docker-compose.yml` | `docker-compose.canary.yml` |
|--------|---------------------|----------------------------|
| **App containers** | 1 (`stellar-spend:latest`) | 2 (primary + canary) |
| **Canary port** | — | `${CANARY_PORT:-3002}` |
| **App image** | Built from `Dockerfile` | `'stellar-spend:${IMAGE_TAG:-latest}'` |
| **Container name** | — | `stellar-spend-canary` |
| **Postgres/Redis** | Same instances | **Shared** with primary stack |
| **Health check** | Full `/api/health` | Same endpoint |

**Usage:**

```bash
# Start primary stack
docker compose up -d

# Start canary on port 3002 using the 'latest' tag
IMAGE_TAG=latest CANARY_PORT=3002 docker compose -f docker-compose.yml -f docker-compose.canary.yml up -d app

# Start canary with a specific image tag
IMAGE_TAG=v1.2.3 CANARY_PORT=3002 docker compose -f docker-compose.yml -f docker-compose.canary.yml up -d app
```

**When to use which:**

- **`docker-compose.yml`**: Local development, staging, and production-equivalent environments. Use this for day-to-day development.
- **`docker-compose.canary.yml`**: Testing a new version of the `app` image against the same database and cache. The canary shares `postgres` and `redis` with the primary stack so data is consistent, but the application code differs. Use this to validate deployments, test image tags, or compare behavior between versions.

> **Important**: The canary compose file must be used **in conjunction with** the primary file (`-f docker-compose.yml -f docker-compose.canary.yml`). It cannot run standalone because it depends on the `postgres` and `redis` services defined in the primary stack.

---

## Contract Build & Test Commands

### npm run audit:contracts

The project provides a unified script to audit all Soroban smart contract dependencies for security vulnerabilities:

```bash
npm run audit:contracts
```

This invokes `./scripts/audit-contracts.sh`, which:

1. Verifies `cargo` is installed
2. Iterates over each crate in `contracts/` (the workspace members: `shared`, `escrow`, `fee-manager`, `multisig-authority`, `treasury`)
3. Runs `cargo audit --deny warnings` on each `Cargo.toml`
4. Reports per-crate results and exits non-zero if any vulnerabilities are found

**Equivalent manual commands:**

```bash
# Audit a specific contract
cd contracts/fee-manager
cargo audit --deny warnings

# Audit all contracts from the workspace root
cd contracts
cargo audit --workspace

# Check for RUSTSEC advisories (JSON output for CI)
cargo audit --json
```

### Building contracts to WebAssembly

```bash
cd contracts
cargo build --target wasm32-unknown-unknown --release
cd ..
```

This compiles all workspace members to WASM binaries under `contracts/*/target/wasm32-unknown-unknown/release/`.

### Running contract tests

```bash
cd contracts
cargo test --workspace
cd ..
```

### Full contract quality pipeline

```bash
# Format
cargo fmt --workspace

# Lint (strict — fails on warnings)
cargo clippy --workspace -- -D warnings

# Security audit
npm run audit:contracts

# Build to WASM
cargo build --target wasm32-unknown-unknown --release

# Test
cargo test --workspace
```

> **Contract workspace members** (from `contracts/Cargo.toml`): `shared`, `escrow`, `fee-manager`, `multisig-authority`, `treasury`. The workspace uses `resolver = "2"` and pins `soroban-sdk = "22"`.

---

## Running the Development Stack

### npm run dev

Starts the Next.js development server:

```bash
npm run dev
```

This runs `node ./scripts/run-next.cjs dev --hostname localhost --port 3001`.

- **Application UI**: [http://localhost:3001](http://localhost:3001)
- **Swagger API Docs**: [http://localhost:3001/api/docs](http://localhost:3001/api/docs)
- **Health check**: `curl http://localhost:3001/api/health`

The dev server watches for changes and supports hot reload. No Docker required for frontend-only development.

### docker-compose up

Starts the full infrastructure stack (app, postgres, redis):

```bash
# Start all services
docker compose up -d

# Start a single service (e.g., postgres only)
docker compose up -d postgres

# Start with migrations
docker compose --profile migrate up -d

# View logs
docker compose logs -f app

# Stop all services
docker compose down
```

The app container uses the `Dockerfile` multi-stage build (`node:20.19.2-alpine3.21`). The first run will pull the base image and build the application. Subsequent runs use the cached `stellar-spend:latest` image.

> **Note**: The `docker-compose.yml` exposes app on port `3000`, not `3001`. Use `docker compose up` for the Docker-based workflow and `npm run dev` for the native Node.js workflow on port `3001`.

### cargo test

Runs the Rust smart contract test suite:

```bash
cd contracts
cargo test --workspace
cd ..
```

This executes all unit and integration tests across every workspace member (`shared`, `escrow`, `fee-manager`, `multisig-authority`, `treasury`).

**Run tests for a specific contract:**

```bash
cd contracts/fee-manager
cargo test
cd ../..
```

**Run tests with output:**

```bash
cargo test --workspace -- --nocapture
```

---

## Complete Setup Walkthrough

Follow these steps to get a fully running local environment from scratch:

### 1. Clone and install Node.js dependencies

```bash
git clone https://github.com/Lex-Studios/Stellar-Spend.git
cd Stellar-Spend
npm install
```

### 2. Install Rust toolchain

```bash
# Install Rust stable if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Add WebAssembly compilation target
rustup target add wasm32-unknown-unknown

# Install cargo-audit for security scanning
cargo install cargo-audit
```

### 3. Configure environment variables

```bash
cp .env.example .env.local
```

Fill in `.env.local` with the required values — see [Environment Variables](./environment-variables.md).

### 4. Start infrastructure via Docker Compose

```bash
docker compose up -d postgres redis
```

Wait for services to be healthy (`docker compose ps` shows `healthy` status).

### 5. Build and test contracts

```bash
# Build all contracts to WASM
cd contracts && cargo build --target wasm32-unknown-unknown --release && cd ..

# Run all contract tests
cd contracts && cargo test --workspace && cd ..

# Audit contract dependencies
npm run audit:contracts
```

### 6. Start the development server

```bash
npm run dev
```

Open [http://localhost:3001](http://localhost:3001).

### 7. (Optional) Run migrations

```bash
docker compose --profile migrate up -d migrate
```

---

## Troubleshooting

### `cargo audit` fails with vulnerabilities

Run `cargo update` in the affected contract directory to pull patched versions, then re-run `npm run audit:contracts`.

### `cargo test --workspace` fails with `error: target 'wasm32-unknown-unknown' not found`

```bash
rustup target add wasm32-unknown-unknown
```

### `docker compose up` fails with port conflicts

- Port `3000` conflicts: another process is using it. Stop it or override with `PORT=3001 docker compose up`.
- Port `5432` conflicts: PostgreSQL is already running locally. Use the native setup instead.
- Port `6379` conflicts: Redis is already running locally.

### `npm run dev` fails with `Invalid environment configuration`

Ensure `.env.local` exists and contains all required variables. Compare against `.env.example`.

### `docker compose` and `npm run dev` port confusion

- `npm run dev` → port **3001** (native Node.js)
- `docker compose up` → port **3000** (Docker container)
- Do not run both simultaneously on the same machine unless using different ports.

### `cargo clippy --workspace -- -D warnings` fails

Fix all Clippy warnings before committing. Format with `cargo fmt --workspace` first, then address remaining warnings. CI enforces this strictly.
