-- Full schema revamp (no prod data to preserve).
-- Replaces legacy KYC orchestration tables with a generic state machine store.
-- All string columns are TEXT (no VARCHAR).

-- Drop legacy tables first (reverse dependency order).
DROP TABLE IF EXISTS phone_deposit CASCADE;
DROP TABLE IF EXISTS kyc_review_decision CASCADE;
DROP TABLE IF EXISTS kyc_review_queue CASCADE;
DROP TABLE IF EXISTS kyc_evidence CASCADE;
DROP TABLE IF EXISTS kyc_upload CASCADE;
DROP TABLE IF EXISTS kyc_magic_email_challenge CASCADE;
DROP TABLE IF EXISTS kyc_otp_challenge CASCADE;
DROP TABLE IF EXISTS kyc_step CASCADE;
DROP TABLE IF EXISTS kyc_session CASCADE;

-- KC/User storage tables (recreated, user_id is external TEXT).
DROP TABLE IF EXISTS device CASCADE;
DROP TABLE IF EXISTS app_user CASCADE;

CREATE TABLE app_user (
  user_id text PRIMARY KEY,
  realm text NOT NULL,
  username text NOT NULL,
  first_name text,
  last_name text,
  email text,
  email_verified boolean NOT NULL DEFAULT false,
  phone_number text,
  fineract_customer_id text,
  disabled boolean NOT NULL DEFAULT false,
  attributes jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX app_user_realm_idx ON app_user(realm);
CREATE INDEX app_user_username_idx ON app_user(username);
CREATE INDEX app_user_phone_number_idx ON app_user(phone_number);
CREATE INDEX app_user_created_at_idx ON app_user(created_at);

-- Device binding:
-- - Composite PK on (device_id, public_jwk) remains (per device-binding safety rule).
-- - Uniqueness enforced on both device_id and jkt.
-- - Deterministic device_record_id is stored for easy lookups and stable references.
CREATE TABLE device (
  device_id text NOT NULL,
  user_id text NOT NULL REFERENCES app_user(user_id),
  jkt text NOT NULL,
  public_jwk text NOT NULL,
  device_record_id text NOT NULL,
  status text NOT NULL,
  label text,
  created_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz,
  PRIMARY KEY (device_id, public_jwk)
);

CREATE UNIQUE INDEX device_device_id_unique ON device(device_id);
CREATE UNIQUE INDEX device_jkt_unique ON device(jkt);
CREATE UNIQUE INDEX device_record_id_unique ON device(device_record_id);
CREATE INDEX device_user_id_idx ON device(user_id);
CREATE INDEX device_last_seen_idx ON device(last_seen_at DESC);

-- Generic state machine persistence (shared by all KYC processes).
CREATE TABLE sm_instance (
  id text PRIMARY KEY,
  kind text NOT NULL,
  user_id text REFERENCES app_user(user_id),
  idempotency_key text NOT NULL,
  status text NOT NULL,
  context jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz
);

CREATE UNIQUE INDEX sm_instance_idempotency_unique ON sm_instance(idempotency_key);
CREATE INDEX sm_instance_kind_status_updated_idx ON sm_instance(kind, status, updated_at DESC);
CREATE INDEX sm_instance_user_kind_updated_idx ON sm_instance(user_id, kind, updated_at DESC);

-- At most one active instance per user + kind.
CREATE UNIQUE INDEX sm_instance_active_unique
  ON sm_instance(user_id, kind)
  WHERE user_id IS NOT NULL AND status IN ('ACTIVE','WAITING_INPUT','RUNNING');

CREATE TABLE sm_event (
  id text PRIMARY KEY,
  instance_id text NOT NULL REFERENCES sm_instance(id) ON DELETE CASCADE,
  kind text NOT NULL,
  actor_type text NOT NULL,
  actor_id text,
  payload jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX sm_event_instance_created_idx ON sm_event(instance_id, created_at ASC);

CREATE TABLE sm_step_attempt (
  id text PRIMARY KEY,
  instance_id text NOT NULL REFERENCES sm_instance(id) ON DELETE CASCADE,
  step_name text NOT NULL,
  attempt_no int NOT NULL,
  status text NOT NULL,
  external_ref text,
  input jsonb NOT NULL DEFAULT '{}'::jsonb,
  output jsonb,
  error jsonb,
  queued_at timestamptz,
  started_at timestamptz,
  finished_at timestamptz,
  next_retry_at timestamptz,
  CONSTRAINT sm_step_attempt_unique UNIQUE(instance_id, step_name, attempt_no)
);

CREATE INDEX sm_step_attempt_instance_step_status_idx ON sm_step_attempt(instance_id, step_name, status);
CREATE INDEX sm_step_attempt_status_retry_idx ON sm_step_attempt(status, next_retry_at);
CREATE INDEX sm_step_attempt_external_ref_idx ON sm_step_attempt(instance_id, step_name, external_ref);

