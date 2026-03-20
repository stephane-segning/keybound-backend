DROP INDEX IF EXISTS app_user_data_eager_fetch_idx;
ALTER TABLE app_user_data DROP COLUMN IF EXISTS eager_fetch;
