-- KYC orchestration schema: sessions + steps + challenges + uploads + evidence + staff review.
-- All string columns are TEXT (no VARCHAR).

CREATE TABLE kyc_session (
  id text PRIMARY KEY,
  user_id text NOT NULL REFERENCES app_user(user_id),
  status text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT kyc_session_status_check CHECK (status IN ('OPEN','COMPLETED','LOCKED')),
  UNIQUE(user_id)
);

CREATE INDEX kyc_session_user_id_idx ON kyc_session(user_id);
CREATE INDEX kyc_session_status_idx ON kyc_session(status);

CREATE TABLE kyc_step (
  id text PRIMARY KEY,
  session_id text NOT NULL REFERENCES kyc_session(id) ON DELETE CASCADE,
  user_id text NOT NULL REFERENCES app_user(user_id),
  type text NOT NULL,
  status text NOT NULL,
  data jsonb NOT NULL DEFAULT '{}'::jsonb,
  policy jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  submitted_at timestamptz,
  CONSTRAINT kyc_step_type_check CHECK (type IN ('PHONE','EMAIL','ADDRESS','IDENTITY')),
  CONSTRAINT kyc_step_status_check CHECK (status IN ('NOT_STARTED','IN_PROGRESS','CAPTURED','PENDING_REVIEW','VERIFIED','REJECTED','FAILED'))
);

CREATE INDEX kyc_step_session_id_idx ON kyc_step(session_id);
CREATE INDEX kyc_step_status_submitted_idx ON kyc_step(status, submitted_at DESC);
CREATE INDEX kyc_step_type_status_idx ON kyc_step(type, status);
CREATE INDEX kyc_step_user_id_idx ON kyc_step(user_id);

CREATE TABLE kyc_otp_challenge (
  otp_ref text PRIMARY KEY,
  step_id text NOT NULL REFERENCES kyc_step(id) ON DELETE CASCADE,
  msisdn text NOT NULL,
  channel text NOT NULL,
  otp_hash text NOT NULL,
  expires_at timestamptz NOT NULL,
  tries_left int NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  verified_at timestamptz,
  CONSTRAINT kyc_otp_channel_check CHECK (channel IN ('SMS','VOICE'))
);

CREATE INDEX kyc_otp_step_id_idx ON kyc_otp_challenge(step_id);
CREATE INDEX kyc_otp_expires_at_idx ON kyc_otp_challenge(expires_at);

CREATE TABLE kyc_magic_email_challenge (
  token_ref text PRIMARY KEY,
  step_id text NOT NULL REFERENCES kyc_step(id) ON DELETE CASCADE,
  email text NOT NULL,
  token_hash text NOT NULL,
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  verified_at timestamptz
);

CREATE INDEX kyc_magic_email_step_id_idx ON kyc_magic_email_challenge(step_id);
CREATE INDEX kyc_magic_email_expires_at_idx ON kyc_magic_email_challenge(expires_at);

CREATE TABLE kyc_upload (
  upload_id text PRIMARY KEY,
  step_id text NOT NULL REFERENCES kyc_step(id) ON DELETE CASCADE,
  user_id text NOT NULL REFERENCES app_user(user_id),
  purpose text NOT NULL,
  asset_type text NOT NULL,
  mime text NOT NULL,
  size_bytes bigint NOT NULL,
  bucket text NOT NULL,
  object_key text NOT NULL,
  method text NOT NULL,
  url text NOT NULL,
  headers jsonb NOT NULL DEFAULT '{}'::jsonb,
  multipart jsonb,
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  etag text,
  computed_sha256 text,
  CONSTRAINT kyc_upload_purpose_check CHECK (purpose IN ('KYC_IDENTITY')),
  CONSTRAINT kyc_upload_asset_type_check CHECK (asset_type IN ('SELFIE_CLOSEUP','SELFIE_WITH_ID','ID_FRONT','ID_BACK'))
);

CREATE INDEX kyc_upload_step_id_idx ON kyc_upload(step_id);
CREATE INDEX kyc_upload_user_id_idx ON kyc_upload(user_id);

CREATE TABLE kyc_evidence (
  evidence_id text PRIMARY KEY,
  step_id text NOT NULL REFERENCES kyc_step(id) ON DELETE CASCADE,
  asset_type text NOT NULL,
  bucket text NOT NULL,
  object_key text NOT NULL,
  sha256 text,
  created_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT kyc_evidence_asset_type_check CHECK (asset_type IN ('SELFIE_CLOSEUP','SELFIE_WITH_ID','ID_FRONT','ID_BACK'))
);

CREATE INDEX kyc_evidence_step_id_idx ON kyc_evidence(step_id);

CREATE TABLE kyc_review_queue (
  id bigserial PRIMARY KEY,
  session_id text NOT NULL REFERENCES kyc_session(id) ON DELETE CASCADE,
  step_id text NOT NULL REFERENCES kyc_step(id) ON DELETE CASCADE,
  status text NOT NULL,
  assigned_to text,
  claimed_at timestamptz,
  lock_expires_at timestamptz,
  priority int NOT NULL DEFAULT 100,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT kyc_review_queue_status_check CHECK (status IN ('PENDING','CLAIMED','DONE')),
  UNIQUE(session_id, step_id)
);

CREATE INDEX kyc_review_queue_status_priority_idx ON kyc_review_queue(status, priority DESC, created_at ASC);
CREATE INDEX kyc_review_queue_session_idx ON kyc_review_queue(session_id);

CREATE TABLE kyc_review_decision (
  id bigserial PRIMARY KEY,
  session_id text NOT NULL REFERENCES kyc_session(id) ON DELETE CASCADE,
  step_id text NOT NULL REFERENCES kyc_step(id) ON DELETE CASCADE,
  outcome text NOT NULL,
  reason_code text NOT NULL,
  comment text,
  decided_at timestamptz NOT NULL DEFAULT now(),
  reviewer_id text,
  CONSTRAINT kyc_review_decision_outcome_check CHECK (outcome IN ('APPROVE','REJECT'))
);

CREATE INDEX kyc_review_decision_step_id_idx ON kyc_review_decision(step_id, decided_at DESC);
