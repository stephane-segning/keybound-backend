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
  source.user_id,
  entry.key AS name,
  'metadata' AS data_type,
  entry.value AS content,
  false AS eager_fetch,
  source.created_at,
  source.updated_at
FROM app_user_data AS source
CROSS JOIN LATERAL jsonb_each(
  CASE
    WHEN jsonb_typeof(source.content) = 'object' THEN source.content
    ELSE '{}'::jsonb
  END
) AS entry(key, value)
WHERE source.name = 'metadata'
  AND source.data_type = 'json'
ON CONFLICT (user_id, name, data_type)
DO UPDATE SET
  content = EXCLUDED.content,
  eager_fetch = app_user_data.eager_fetch,
  updated_at = GREATEST(app_user_data.updated_at, EXCLUDED.updated_at);

DELETE FROM app_user_data
WHERE name = 'metadata'
  AND data_type = 'json'
  AND jsonb_typeof(content) = 'object';
