ALTER TABLE app_user
  ADD COLUMN IF NOT EXISTS fineract_customer_id TEXT;

CREATE INDEX IF NOT EXISTS idx_user_fineract
  ON app_user(fineract_customer_id);
