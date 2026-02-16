UPDATE kyc_submission
SET
  status = 'SUBMITTED',
  submitted_at = now(),
  updated_at = now()
FROM kyc_case kc
WHERE kyc_submission.kyc_case_id = kc.id
  AND kc.user_id = $2
  AND kyc_submission.id = $1
  AND kyc_submission.status = 'DRAFT'
