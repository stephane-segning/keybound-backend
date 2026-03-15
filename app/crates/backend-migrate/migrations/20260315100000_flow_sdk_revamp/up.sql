ALTER TABLE app_user
  ADD COLUMN IF NOT EXISTS metadata JSONB NOT NULL DEFAULT '{}';

CREATE INDEX IF NOT EXISTS idx_app_user_metadata
  ON app_user USING GIN (metadata);

CREATE TABLE IF NOT EXISTS flow_session (
  id TEXT PRIMARY KEY,
  human_id TEXT UNIQUE NOT NULL,
  user_id TEXT REFERENCES app_user(user_id),
  session_type TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'OPEN',
  context JSONB NOT NULL DEFAULT '{}',
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  completed_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS flow_instance (
  id TEXT PRIMARY KEY,
  human_id TEXT UNIQUE NOT NULL,
  session_id TEXT NOT NULL REFERENCES flow_session(id) ON DELETE CASCADE,
  flow_type TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'PENDING',
  current_step TEXT,
  step_ids JSONB NOT NULL DEFAULT '[]',
  context JSONB NOT NULL DEFAULT '{}',
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS flow_step (
  id TEXT PRIMARY KEY,
  human_id TEXT UNIQUE NOT NULL,
  flow_id TEXT NOT NULL REFERENCES flow_instance(id) ON DELETE CASCADE,
  step_type TEXT NOT NULL,
  actor TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'PENDING',
  attempt_no INT NOT NULL DEFAULT 0,
  input JSONB,
  output JSONB,
  error JSONB,
  next_retry_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  finished_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS signing_key (
  kid TEXT PRIMARY KEY,
  private_key_pem TEXT NOT NULL,
  public_key_jwk JSONB NOT NULL,
  algorithm TEXT NOT NULL DEFAULT 'RS256',
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  expires_at TIMESTAMPTZ,
  is_active BOOL NOT NULL DEFAULT TRUE
);

CREATE INDEX IF NOT EXISTS idx_flow_session_user ON flow_session(user_id);
CREATE INDEX IF NOT EXISTS idx_flow_instance_session ON flow_instance(session_id);
CREATE INDEX IF NOT EXISTS idx_flow_step_flow ON flow_step(flow_id);
CREATE INDEX IF NOT EXISTS idx_flow_step_status ON flow_step(status);
CREATE INDEX IF NOT EXISTS idx_signing_key_active ON signing_key(is_active) WHERE is_active = TRUE;

INSERT INTO flow_session (id, human_id, user_id, session_type, status, context, created_at, updated_at, completed_at)
SELECT
  id,
  'legacy.' || id,
  user_id,
  kind,
  status,
  context,
  created_at,
  updated_at,
  completed_at
FROM sm_instance
ON CONFLICT (id) DO NOTHING;

INSERT INTO flow_instance (id, human_id, session_id, flow_type, status, current_step, step_ids, context, created_at, updated_at)
SELECT
  'flw_' || id,
  'legacy.' || id || '.' || kind,
  id,
  kind,
  status,
  context ->> 'current_step',
  CASE
    WHEN jsonb_typeof(context -> 'step_ids') = 'array' THEN context -> 'step_ids'
    ELSE '[]'::jsonb
  END,
  context,
  created_at,
  updated_at
FROM sm_instance
ON CONFLICT (id) DO NOTHING;

INSERT INTO flow_step (id, human_id, flow_id, step_type, actor, status, attempt_no, input, output, error, next_retry_at, created_at, updated_at, finished_at)
SELECT
  'stp_' || id,
  'legacy.' || instance_id || '.' || step_name,
  'flw_' || instance_id,
  step_name,
  UPPER(CASE
    WHEN actor_type = 'staff' THEN 'ADMIN'
    WHEN actor_type = 'user' THEN 'END_USER'
    ELSE 'SYSTEM'
  END),
  status,
  attempt_no,
  input,
  output,
  error,
  next_retry_at,
  COALESCE(queued_at, NOW()),
  COALESCE(started_at, COALESCE(queued_at, NOW())),
  finished_at
FROM sm_step_attempt a
LEFT JOIN LATERAL (
  SELECT e.actor_type
  FROM sm_event e
  WHERE e.instance_id = a.instance_id
    AND e.kind ILIKE '%' || a.step_name || '%'
  ORDER BY e.created_at DESC
  LIMIT 1
) actor_info ON TRUE
ON CONFLICT (id) DO NOTHING;
