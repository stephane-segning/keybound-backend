WITH user_case AS (
  SELECT id FROM kyc_case WHERE user_id = $1
)
INSERT INTO kyc_submission (id, kyc_case_id, version, status)
SELECT 
  'sub_' || lower(hex(random_bytes(16))), -- Fallback if backend-id is not used here, but usually handled by app
  id,
  1,
  'DRAFT'
FROM user_case
WHERE NOT EXISTS (
  SELECT 1 FROM kyc_submission WHERE kyc_case_id = user_case.id
)
