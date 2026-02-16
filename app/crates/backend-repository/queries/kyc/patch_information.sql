UPDATE kyc_submission
SET
  first_name = COALESCE($3, first_name),
  last_name = COALESCE($4, last_name),
  email = COALESCE($5, email),
  phone_number = COALESCE($6, phone_number),
  date_of_birth = COALESCE($7, date_of_birth),
  nationality = COALESCE($8, nationality),
  updated_at = now()
FROM kyc_case kc
WHERE kyc_submission.kyc_case_id = kc.id
  AND kc.user_id = $1
  AND kyc_submission.status = 'DRAFT'
  AND ($2::int IS NULL OR kyc_submission.version = $2)
RETURNING
  kyc_submission.id as submission_id,
  kc.user_id as external_id,
  kyc_submission.first_name,
  kyc_submission.last_name,
  kyc_submission.email,
  kyc_submission.phone_number,
  kyc_submission.date_of_birth,
  kyc_submission.nationality,
  kc.current_tier as kyc_tier,
  kyc_submission.status::text as kyc_status,
  kyc_submission.submitted_at,
  kyc_submission.decided_at as reviewed_at,
  kyc_submission.decided_by as reviewed_by,
  kyc_submission.rejection_reason,
  kyc_submission.review_notes,
  kyc_submission.created_at,
  kyc_submission.updated_at,
  kyc_submission.version
