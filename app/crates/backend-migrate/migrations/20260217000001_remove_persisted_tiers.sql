-- Remove persisted tier columns from kyc_case and kyc_submission

ALTER TABLE kyc_case DROP COLUMN current_tier;
ALTER TABLE kyc_submission DROP COLUMN requested_tier;
ALTER TABLE kyc_submission DROP COLUMN decided_tier;
