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
  'metadata' AS name,
  'json' AS data_type,
  jsonb_object_agg(name, content) AS content,
  false AS eager_fetch,
  MIN(created_at) AS created_at,
  MAX(updated_at) AS updated_at
FROM app_user_data
WHERE data_type = 'metadata'
GROUP BY user_id
ON CONFLICT (user_id, name, data_type)
DO UPDATE SET
  content = EXCLUDED.content,
  updated_at = GREATEST(app_user_data.updated_at, EXCLUDED.updated_at);

DELETE FROM app_user_data
WHERE data_type = 'metadata';
