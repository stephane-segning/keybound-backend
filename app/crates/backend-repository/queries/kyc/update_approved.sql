WITH updated_submission AS (
  UPDATE kyc_submission
  SET
    status = 'APPROVED',
    decided_tier = $2,
    decided_at = now(),
    decided_by = 'staff',
    review_notes = $3,
    updated_at = now()
  FROM kyc_case kc
  WHERE kyc_submission.kyc_case_id = kc.id
    AND kc.user_id = $1
    AND kyc_submission.id = kc.active_submission_id
  RETURNING kyc_submission.kyc_case_id
)
UPDATE kyc_case
SET
  current_tier = $2,
  updated_at = now()
WHERE id IN (SELECT kyc_case_id FROM updated_submission)
   OR (user_id = $1 AND EXISTS (SELECT 1 FROM updated_submission))
