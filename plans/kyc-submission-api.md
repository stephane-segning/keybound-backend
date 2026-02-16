# KYC Submission API Design Proposal

## Recommendation
Use the existing BFF endpoint [`POST /api/kyc/submissions/{submissionId}:submit`](openapi/user-storage-bff.yaml:74) for KYC submission, with no new endpoint added.

## Rationale
- The current BFF OpenAPI already defines a dedicated submission action endpoint for the customer surface ([`openapi/user-storage-bff.yaml`](openapi/user-storage-bff.yaml:74)), matching the submission intent and avoiding duplication.
- The endpoint is scoped to a specific submission resource and models a state transition (DRAFT -> SUBMITTED), which aligns with the action-style `:submit` suffix used elsewhere in the spec.
- The endpoint already supports idempotency via `Idempotency-Key` and returns the canonical submission detail response, which is exactly what the client needs after a submission action.
- The registration surface has a JSON Patch endpoint for profile updates ([`PATCH /api/registration/kyc/profile`](openapi/user-storage-bff.yaml:163)) but no submission action, so introducing a new endpoint under `/api/registration/kyc/...` would duplicate the existing submission action and fragment the KYC flow across two base paths.

## API Definition
No new endpoint is proposed. The existing endpoint is the canonical submission API.

### Existing Endpoint (Canonical)
```yaml
/api/kyc/submissions/{submissionId}:submit:
  post:
    tags: [ KYC ]
    summary: Submit a KYC submission
    description: >
      Submits the submission for review. After submission, profile fields become read-only until staff requests more info.
      Idempotency-Key is recommended for safe retries.
    security: [ { bearerAuth: [ ] } ]
    parameters:
      - $ref: '#/components/parameters/SubmissionId'
      - $ref: '#/components/parameters/IdempotencyKey'
    responses:
      '200':
        description: Submission submitted.
        content:
          application/json:
            schema: { $ref: '#/components/schemas/KycSubmissionDetailResponse' }
      '400': { $ref: '#/components/responses/BadRequest' }
      '401': { $ref: '#/components/responses/Unauthorized' }
      '404': { $ref: '#/components/responses/NotFound' }
      '409': { $ref: '#/components/responses/Conflict' }
```

## Notes
- If any client path standardization is needed later, prefer redirecting or aliasing to this endpoint rather than adding a new submission API under `/api/registration/kyc/...`.
