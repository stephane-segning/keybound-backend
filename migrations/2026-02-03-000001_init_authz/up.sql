-- Base schema for user-storage backend (KC + BFF + Staff).
-- This migration intentionally replaces the previous placeholder schema.

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'device_status') THEN
    CREATE TYPE device_status AS ENUM ('ACTIVE', 'REVOKED', 'SUSPENDED');
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'approval_status') THEN
    CREATE TYPE approval_status AS ENUM ('PENDING', 'APPROVED', 'DENIED', 'EXPIRED');
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'sms_status') THEN
    CREATE TYPE sms_status AS ENUM ('PENDING', 'SENT', 'FAILED', 'GAVE_UP');
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'kyc_status') THEN
    CREATE TYPE kyc_status AS ENUM ('PENDING', 'APPROVED', 'REJECTED', 'NEEDS_INFO');
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'kyc_document_status') THEN
    CREATE TYPE kyc_document_status AS ENUM (
      'PRESIGNED',
      'UPLOADED',
      'UNDER_REVIEW',
      'APPROVED',
      'REJECTED'
    );
  END IF;
END $$;

-- Keycloak user-storage canonical record.
CREATE TABLE IF NOT EXISTS users (
  user_id TEXT PRIMARY KEY,
  realm TEXT NOT NULL,
  username TEXT NOT NULL,
  first_name TEXT NULL,
  last_name TEXT NULL,
  email TEXT NULL,
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  email_verified BOOLEAN NOT NULL DEFAULT FALSE,
  attributes JSONB NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_realm_username_unique
  ON users (realm, username);
CREATE INDEX IF NOT EXISTS idx_users_realm_email
  ON users (realm, email);

-- Device registry and binding.
CREATE TABLE IF NOT EXISTS devices (
  id TEXT PRIMARY KEY,
  realm TEXT NOT NULL,
  client_id TEXT NOT NULL,
  user_id TEXT NOT NULL REFERENCES users (user_id) ON DELETE CASCADE,
  user_hint TEXT NULL,
  device_id TEXT NOT NULL,
  jkt TEXT NOT NULL,
  status device_status NOT NULL DEFAULT 'ACTIVE',
  public_jwk JSONB NOT NULL,
  attributes JSONB NULL,
  proof JSONB NULL,
  label TEXT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_seen_at TIMESTAMPTZ NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_devices_device_id_unique
  ON devices (device_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_devices_jkt_unique
  ON devices (jkt);
CREATE INDEX IF NOT EXISTS idx_devices_user_status
  ON devices (user_id, status);

-- Approval workflow for new-device binding.
CREATE TABLE IF NOT EXISTS approvals (
  request_id TEXT PRIMARY KEY,
  realm TEXT NOT NULL,
  client_id TEXT NOT NULL,
  user_id TEXT NOT NULL REFERENCES users (user_id) ON DELETE CASCADE,
  device_id TEXT NOT NULL,
  jkt TEXT NOT NULL,
  public_jwk JSONB NULL,
  platform TEXT NULL,
  model TEXT NULL,
  app_version TEXT NULL,
  reason TEXT NULL,
  expires_at TIMESTAMPTZ NULL,
  context JSONB NULL,
  idempotency_key TEXT NULL,
  status approval_status NOT NULL DEFAULT 'PENDING',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  decided_at TIMESTAMPTZ NULL,
  decided_by_device_id TEXT NULL,
  message TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_approvals_user_status_created
  ON approvals (user_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_approvals_expires_at
  ON approvals (expires_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_approvals_idempotency_unique
  ON approvals (realm, client_id, idempotency_key)
  WHERE idempotency_key IS NOT NULL;

-- SMS send/confirm with persisted retry state (SNS integration happens in app code).
CREATE TABLE IF NOT EXISTS sms_messages (
  id TEXT PRIMARY KEY,
  realm TEXT NOT NULL,
  client_id TEXT NOT NULL,
  user_id TEXT NULL REFERENCES users (user_id) ON DELETE SET NULL,
  phone_number TEXT NOT NULL,
  hash TEXT NOT NULL,
  otp_sha256 BYTEA NOT NULL,
  ttl_seconds INT NULL,
  status sms_status NOT NULL DEFAULT 'PENDING',
  attempt_count INT NOT NULL DEFAULT 0,
  max_attempts INT NOT NULL,
  next_retry_at TIMESTAMPTZ NULL,
  last_error TEXT NULL,
  sns_message_id TEXT NULL,
  session_id TEXT NULL,
  trace_id TEXT NULL,
  metadata JSONB NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  sent_at TIMESTAMPTZ NULL,
  confirmed_at TIMESTAMPTZ NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_sms_messages_hash_unique
  ON sms_messages (hash);
CREATE INDEX IF NOT EXISTS idx_sms_messages_retry
  ON sms_messages (status, next_retry_at);
CREATE INDEX IF NOT EXISTS idx_sms_messages_phone_created
  ON sms_messages (phone_number, created_at DESC);

-- KYC aggregate profile (staff + customer-facing read models).
CREATE TABLE IF NOT EXISTS kyc_profiles (
  external_id TEXT PRIMARY KEY,
  first_name TEXT NULL,
  last_name TEXT NULL,
  email TEXT NULL,
  phone_number TEXT NULL,
  date_of_birth TEXT NULL,
  nationality TEXT NULL,
  kyc_tier INT NOT NULL DEFAULT 0,
  kyc_status kyc_status NOT NULL DEFAULT 'PENDING',
  submitted_at TIMESTAMPTZ NULL,
  reviewed_at TIMESTAMPTZ NULL,
  reviewed_by TEXT NULL,
  rejection_reason TEXT NULL,
  review_notes TEXT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_kyc_profiles_status_submitted
  ON kyc_profiles (kyc_status, submitted_at DESC);
CREATE INDEX IF NOT EXISTS idx_kyc_profiles_email
  ON kyc_profiles (email);
CREATE INDEX IF NOT EXISTS idx_kyc_profiles_phone
  ON kyc_profiles (phone_number);

-- Individual KYC documents stored via S3 (presign intent).
CREATE TABLE IF NOT EXISTS kyc_documents (
  id TEXT PRIMARY KEY,
  external_id TEXT NOT NULL REFERENCES kyc_profiles (external_id) ON DELETE CASCADE,
  document_type TEXT NOT NULL,
  status kyc_document_status NOT NULL DEFAULT 'PRESIGNED',
  uploaded_at TIMESTAMPTZ NULL,
  rejection_reason TEXT NULL,
  file_name TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  content_length BIGINT NOT NULL,
  s3_bucket TEXT NOT NULL,
  s3_key TEXT NOT NULL,
  presigned_expires_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_kyc_documents_s3_key_unique
  ON kyc_documents (s3_key);
CREATE INDEX IF NOT EXISTS idx_kyc_documents_external_status
  ON kyc_documents (external_id, status, created_at DESC);
