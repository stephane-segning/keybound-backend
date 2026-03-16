#!/bin/bash
# Full Demo Script for KYC Tokenization Backend
# Demonstrates: Signature Auth, Phone OTP Flow, First Deposit Flow

set -e

echo "========================================"
echo "  KYC Tokenization Backend Demo"
echo "========================================"
echo ""

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Check prerequisites
echo -e "${YELLOW}1. Checking prerequisites...${NC}"
command -v docker >/dev/null 2>&1 || { echo -e "${RED}docker is required${NC}"; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo -e "${RED}cargo is required${NC}"; exit 1; }
echo -e "${GREEN}   ✓ Docker and Cargo available${NC}"
echo ""

# Run unit tests
echo -e "${YELLOW}2. Running unit tests...${NC}"
cargo test --workspace --lib --quiet
echo -e "${GREEN}   ✓ All unit tests passed${NC}"
echo ""

# Run Rust E2E tests
echo -e "${YELLOW}3. Running Rust E2E tests (wiremock)...${NC}"
just test-e2e-rust
echo -e "${GREEN}   ✓ Rust E2E tests passed${NC}"
echo ""

# Run full stack E2E
echo -e "${YELLOW}4. Running full stack E2E tests...${NC}"
echo "   This spins up: Postgres, Redis, MinIO, Keycloak, SMS Gateway, Backend Server"
echo ""
just test-e2e-smoke
echo -e "${GREEN}   ✓ Smoke tests passed${NC}"
echo ""

echo "========================================"
echo -e "${GREEN}Demo Complete!${NC}"
echo "========================================"
echo ""
echo "What was demonstrated:"
echo "  ✓ Signature Auth: Device-bound cryptographic signatures (P-256 ES256)"
echo "  ✓ Replay Protection: Redis-backed nonce validation"
echo "  ✓ OAuth JWT: Keycloak integration for staff actions"
echo "  ✓ Flow SDK: Phone OTP and First Deposit flows"
echo "  ✓ External Webhooks: Payment processor integration"
echo ""
echo "To run the full E2E suite:"
echo "  just test-e2e-full"
echo ""
echo "To start the backend locally for manual testing:"
echo "  just dev serve --config-path config/dev.yaml"