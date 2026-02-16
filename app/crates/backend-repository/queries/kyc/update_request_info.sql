UPDATE kyc_submission
SET
  status = 'PENDING_USER_RESPONSE',
  decided_at = now(),
  decided_by = 'staff',
  review_notes = $2,
  updated_at = now()
FROM kyc_case kc
WHERE kyc_submission.kyc_case_id = kc.id
  AND kc.user_id = $1
  AND kyc_submission.id = kc.active_submission_id
