ALTER TABLE app_user
  ADD COLUMN IF NOT EXISTS metadata JSONB NOT NULL DEFAULT '{}';

UPDATE app_user
SET metadata = app_user_data.content
FROM app_user_data
WHERE app_user.user_id = app_user_data.user_id
  AND app_user_data.name = 'metadata'
  AND app_user_data.data_type = 'json';

CREATE INDEX IF NOT EXISTS idx_app_user_metadata
  ON app_user USING GIN (metadata);
