SELECT
  id,
  external_id,
  document_type,
  status::text as status,
  uploaded_at,
  rejection_reason,
  file_name,
  mime_type,
  content_length,
  s3_bucket,
  s3_key,
  presigned_expires_at,
  created_at,
  updated_at
FROM kyc_documents
WHERE id = @id AND external_id = @external_id
