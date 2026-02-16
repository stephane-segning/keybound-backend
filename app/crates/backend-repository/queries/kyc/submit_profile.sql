UPDATE kyc_profiles
SET
  kyc_status = 'SUBMITTED',
  submitted_at = now(),
  updated_at = now()
WHERE
  external_id = $2
  AND ('sub_' || external_id) = $1
