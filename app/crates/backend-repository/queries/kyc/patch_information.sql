UPDATE kyc_profiles
SET
  first_name = COALESCE($3, first_name),
  last_name = COALESCE($4, last_name),
  email = COALESCE($5, email),
  phone_number = COALESCE($6, phone_number),
  date_of_birth = COALESCE($7, date_of_birth),
  nationality = COALESCE($8, nationality),
  updated_at = now(),
  version = version + 1
WHERE external_id = $1
  AND ($2::int IS NULL OR version = $2)
RETURNING
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
