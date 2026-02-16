-- Consolidate KYC data into kyc_submission and remove redundant kyc_profiles table

ALTER TABLE kyc_submission
  ADD COLUMN first_name varchar(255),
  ADD COLUMN last_name varchar(255),
  ADD COLUMN email varchar(320),
  ADD COLUMN phone_number varchar(64),
  ADD COLUMN date_of_birth varchar(64),
  ADD COLUMN nationality varchar(128),
  ADD COLUMN rejection_reason text,
  ADD COLUMN review_notes text;

-- Drop the redundant table and type
DROP TABLE IF EXISTS kyc_profiles;
DROP TYPE IF EXISTS kyc_status;
