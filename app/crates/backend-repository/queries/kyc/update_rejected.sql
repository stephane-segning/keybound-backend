UPDATE kyc_submission
SET
  status = 'REJECTED',
  decided_at = now(),
  decided_by = 'staff',
  rejection_reason = $2,
  review_notes = $3,
  updated_at = now()
FROM kyc_case kc
WHERE kyc_submission.kyc_case_id = kc.id
  AND kc.user_id = $1
  AND kyc_submission.id = kc.active_submission_id
