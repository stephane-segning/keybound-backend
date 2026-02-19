CREATE TYPE kyc_case_status AS ENUM ('OPEN','CLOSED');

CREATE TYPE kyc_submission_status AS ENUM (
  'DRAFT',
  'SUBMITTED',
  'IN_VERIFICATION',
  'RISK_ASSESSMENT',
  'PENDING_MANUAL_REVIEW',
  'PENDING_USER_RESPONSE',
  'APPROVED',
  'REJECTED'
);

CREATE TYPE kyc_provisioning_status AS ENUM (
  'NONE',
  'STARTED',
  'SUCCEEDED',
  'FAILED'
);

CREATE TABLE kyc_case (
  id text PRIMARY KEY,
  user_id text NOT NULL REFERENCES app_user(user_id),
  current_tier int NOT NULL DEFAULT 0,
  case_status kyc_case_status NOT NULL DEFAULT 'OPEN',
  active_submission_id text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(user_id)
);

CREATE TABLE kyc_submission (
  id text PRIMARY KEY,
  kyc_case_id text NOT NULL REFERENCES kyc_case(id) ON DELETE CASCADE,
  status kyc_submission_status NOT NULL DEFAULT 'DRAFT',
  requested_tier int NOT NULL DEFAULT 1,
  decided_tier int,
  submitted_at timestamptz,
  decided_at timestamptz,
  decided_by text,
  provisioning_status kyc_provisioning_status NOT NULL DEFAULT 'NONE',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
);

ALTER TABLE kyc_case
  ADD CONSTRAINT fk_active_submission
  FOREIGN KEY (active_submission_id)
  REFERENCES kyc_submission(id);

CREATE INDEX idx_kyc_case_active_submission ON kyc_case(active_submission_id);
CREATE INDEX idx_kyc_submission_status ON kyc_submission(status);
