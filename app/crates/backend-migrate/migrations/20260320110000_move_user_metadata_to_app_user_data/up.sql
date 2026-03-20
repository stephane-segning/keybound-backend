INSERT INTO app_user_data (
  user_id,
  name,
  data_type,
  content,
  eager_fetch,
  created_at,
  updated_at
)
SELECT
  user_id,
  'metadata',
  'json',
  metadata,
  false,
  created_at,
  updated_at
FROM app_user
ON CONFLICT (user_id, name, data_type)
DO UPDATE SET
  content = EXCLUDED.content,
  updated_at = GREATEST(app_user_data.updated_at, EXCLUDED.updated_at);

DROP INDEX IF EXISTS idx_app_user_metadata;
ALTER TABLE app_user DROP COLUMN IF EXISTS metadata;
