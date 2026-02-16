SELECT
  ks.id as submission_id,
  kc.user_id as external_id,
  ks.first_name,
  ks.last_name,
  ks.email,
  ks.phone_number,
  ks.date_of_birth,
  ks.nationality,
  kc.current_tier as kyc_tier,
  ks.status::text as kyc_status,
  ks.submitted_at,
  ks.decided_at as reviewed_at,
  ks.decided_by as reviewed_by,
  ks.rejection_reason,
  ks.review_notes,
  ks.created_at,
  ks.updated_at,
  ks.version
FROM kyc_submission ks
JOIN kyc_case kc ON ks.kyc_case_id = kc.id
WHERE ks.status != 'DRAFT'
ORDER BY ks.submitted_at DESC NULLS LAST, ks.created_at DESC
