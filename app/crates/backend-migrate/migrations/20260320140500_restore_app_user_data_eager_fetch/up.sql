ALTER TABLE app_user_data
  ADD COLUMN IF NOT EXISTS eager_fetch BOOLEAN NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS app_user_data_eager_fetch_idx
  ON app_user_data(user_id, eager_fetch);
