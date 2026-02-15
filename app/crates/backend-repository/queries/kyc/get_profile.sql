SELECT
  external_id,
  first_name,
  last_name,
  email,
  phone_number,
  date_of_birth,
  nationality,
  kyc_tier,
  kyc_status::text as kyc_status,
  submitted_at,
  reviewed_at,
  reviewed_by,
  rejection_reason,
  review_notes,
  created_at,
  updated_at,
  version
FROM kyc_profiles
WHERE external_id = $1
