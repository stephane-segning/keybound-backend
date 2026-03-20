DROP INDEX IF EXISTS idx_user_fineract;
ALTER TABLE app_user DROP COLUMN IF EXISTS fineract_customer_id;
