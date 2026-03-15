DROP TABLE IF EXISTS flow_step;
DROP TABLE IF EXISTS flow_instance;
DROP TABLE IF EXISTS flow_session;
DROP TABLE IF EXISTS signing_key;

DROP INDEX IF EXISTS idx_app_user_metadata;
ALTER TABLE app_user DROP COLUMN IF EXISTS metadata;
