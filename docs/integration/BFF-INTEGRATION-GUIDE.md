# BFF Integration Guide

## Overview

The BFF (Backend for Frontend) API is the primary interface for KYC orchestration. All endpoints require **signature
authentication** with device-bound cryptographic keys.

---

## Authentication

### Required Headers

Every BFF request must include:

| Header                       | Description                                |
|------------------------------|--------------------------------------------|
| `x-auth-device-id`           | Device identifier (from device enrollment) |
| `x-auth-signature-timestamp` | Unix timestamp (seconds)                   |
| `x-auth-public-key`          | JWK public key JSON                        |
| `x-auth-nonce`               | Random unique string                       |
| `x-auth-signature`           | ES256 signature (base64url)                |

### Canonical Payload Format

```
{timestamp}
{nonce}
{METHOD}
{path}
{body}
{public_jwk}
{device_id}
{user_id_hint}
```

### Canonical Payload Format

**Device Authentication Payload (used by frontend/BFF):**

```json
{
  "deviceId": "dvc_xxx",
  "nonce": "nce_xxx",
  "publicKey": "{\"crv\":\"P-256\",\"kty\":\"EC\",\"x\":\"...\",\"y\":\"...\"}",
  "ts": "1710000000"
}
```

**Key Order:** Alphabetical (`deviceId`, `nonce`, `publicKey`, `ts`)

**Important:** The `publicKey` value must be escaped when embedded in JSON:
```javascript
const escapedPublicKey = publicKey.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
const canonical = `{"deviceId":"${deviceId}","nonce":"${nonce}","publicKey":"${escapedPublicKey}","ts":"${ts}"}`;
```

### Signing Example (JavaScript/WebCrypto)

```javascript
async function signRequest(privateKey, timestamp, nonce, method, path, body, deviceId, userId) {
    const publicKey = await crypto.subtle.exportKey("jwk", privateKey.publicKey);

    const canonical = [
        timestamp.toString(),
        nonce,
        method.toUpperCase(),
        path,
        body || '',
        JSON.stringify(publicKey),
        deviceId,
        userId || ''
    ].join('\n');

    const encoder = new TextEncoder();
    const data = encoder.encode(canonical);

    const signature = await crypto.subtle.sign(
        {name: "ECDSA", hash: "SHA-256"},
        privateKey,
        data
    );

    return btoa(String.fromCharCode(...new Uint8Array(signature)))
        .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}
```

---

## API Model

### Hierarchy

```
Session (KYC journey container)
├── Flow (specific KYC process)
│   ├── Step (individual action)
│   ├── Step (next action)
│   └── ...
├── Flow (another process in same session)
│   └── ...
└── ...
```

### Resource IDs

| Resource | ID Format  | Human ID Format |
|----------|------------|-----------------|
| Session  | `sess_xxx` | `SESS-xxx`      |
| Flow     | `flw_xxx`  | `PHONE-OTP-xxx` |
| Step     | `stp_xxx`  | `VERIFY-xxx`    |

---

## Flow 1: Phone OTP Verification

### Purpose

Verify user's phone number ownership via SMS OTP.

### Sequence

```
┌─────────────────────────────────────────────────────────────┐
│ 1. GET /users/{userId}                                      │
│    → Get user profile (phone number from Keycloak)          │
├─────────────────────────────────────────────────────────────┤
│ 2. POST /sessions                                           │
│    → Create KYC session                                     │
├─────────────────────────────────────────────────────────────┤
│ 3. POST /sessions/{sessionId}/flows                         │
│    → Add PHONE_OTP flow to session                          │
├─────────────────────────────────────────────────────────────┤
│ 4. GET /flows/{flowId}                                      │
│    → Get flow status, find current step                     │
├─────────────────────────────────────────────────────────────┤
│ 5. POST /steps/{stepId}                                     │
│    → Submit step input (OTP code, trigger SMS, etc.)        │
├─────────────────────────────────────────────────────────────┤
│ 6. Repeat step 4-5 until flow status = COMPLETED            │
└─────────────────────────────────────────────────────────────┘
```

### Step 1: Get User Profile

```http
GET /bff/users/usr_abc123 HTTP/1.1
Host: kyc.example.com
x-auth-device-id: dvc_xyz789
x-auth-signature-timestamp: 1710000000
x-auth-public-key: {"kty":"EC","crv":"P-256","x":"...","y":"..."}
x-auth-nonce: nonce_abc123
x-auth-signature: signature_base64url
```

**Response:**

```json
{
  "userId": "usr_abc123",
  "realm": "e2e-testing",
  "username": "user123",
  "phoneNumber": "+237690123456",
  "emailVerified": true,
  "disabled": false,
  "createdAt": "2024-01-01T00:00:00Z",
  "updatedAt": "2024-01-01T00:00:00Z"
}
```

### Step 2: Create Session

```http
POST /bff/sessions HTTP/1.1
Content-Type: application/json

{
  "sessionType": "KYC_ONBOARDING",
  "humanId": "ONBOARD-2024-001"
}
```

**Response:**

```json
{
  "id": "sess_def456",
  "humanId": "ONBOARD-2024-001",
  "sessionType": "KYC_ONBOARDING",
  "status": "ACTIVE",
  "userId": "usr_abc123",
  "context": {},
  "createdAt": "2024-03-15T10:00:00Z",
  "updatedAt": "2024-03-15T10:00:00Z"
}
```

### Step 3: Add Phone OTP Flow

```http
POST /bff/sessions/sess_def456/flows HTTP/1.1
Content-Type: application/json

{
  "flowType": "PHONE_OTP",
  "humanId": "PHONE-OTP-001"
}
```

**Response:**

```json
{
  "id": "flw_ghi789",
  "humanId": "PHONE-OTP-001",
  "sessionId": "sess_def456",
  "flowType": "PHONE_OTP",
  "status": "RUNNING",
  "currentStep": "SEND_SMS",
  "stepIds": [
    "stp_send",
    "stp_verify"
  ],
  "context": {
    "phoneNumber": "+237690123456"
  },
  "createdAt": "2024-03-15T10:00:00Z",
  "updatedAt": "2024-03-15T10:00:00Z"
}
```

### Step 4: Get Flow Details

```http
GET /bff/flows/flw_ghi789 HTTP/1.1
```

**Response:**

```json
{
  "flow": {
    "id": "flw_ghi789",
    "status": "RUNNING",
    "currentStep": "VERIFY_OTP",
    "context": {
      "phoneNumber": "+237690123456",
      "smsSent": true
    }
  },
  "steps": [
    {
      "id": "stp_send",
      "stepType": "SEND_SMS",
      "status": "COMPLETED",
      "output": {
        "messageId": "msg_123"
      }
    },
    {
      "id": "stp_verify",
      "stepType": "VERIFY_OTP",
      "status": "PENDING_INPUT",
      "input": null
    }
  ]
}
```

### Step 5: Submit OTP

```http
POST /bff/steps/stp_verify HTTP/1.1
Content-Type: application/json

{
  "input": {
    "otpCode": "123456"
  }
}
```

**Response (Success):**

```json
{
  "id": "stp_verify",
  "stepType": "VERIFY_OTP",
  "status": "COMPLETED",
  "output": {
    "verified": true,
    "verifiedAt": "2024-03-15T10:05:00Z"
  }
}
```

**Response (Failure):**

```json
{
  "id": "stp_verify",
  "stepType": "VERIFY_OTP",
  "status": "FAILED",
  "error": {
    "code": "INVALID_OTP",
    "message": "OTP code is invalid or expired"
  }
}
```

---

## Flow 2: First Deposit

### Purpose

Process user's first deposit with admin approval.

### Sequence

```
┌─────────────────────────────────────────────────────────────┐
│ 1. POST /sessions (or reuse existing)                       │
├─────────────────────────────────────────────────────────────┤
│ 2. POST /sessions/{sessionId}/flows                         │
│    → Add FIRST_DEPOSIT flow                                 │
├─────────────────────────────────────────────────────────────┤
│ 3. GET /flows/{flowId}                                      │
│    → Check current step                                     │
├─────────────────────────────────────────────────────────────┤
│ 4. POST /steps/{stepId} (USER_SUBMITS)                      │
│    → Submit deposit details (amount, currency, receipt)     │
├─────────────────────────────────────────────────────────────┤
│ 5. Poll GET /flows/{flowId}                                 │
│    → Wait for ADMIN_APPROVES step (status changes)          │
├─────────────────────────────────────────────────────────────┤
│ 6. Flow status = COMPLETED when approved                    │
│    OR FAILED if rejected                                    │
└─────────────────────────────────────────────────────────────┘
```

### Step 1: Create Session (or reuse)

```http
POST /bff/sessions HTTP/1.1
Content-Type: application/json

{
  "sessionType": "KYC_ONBOARDING",
  "humanId": "ONBOARD-2024-002"
}
```

### Step 2: Add First Deposit Flow

```http
POST /bff/sessions/sess_def456/flows HTTP/1.1
Content-Type: application/json

{
  "flowType": "FIRST_DEPOSIT",
  "humanId": "DEPOSIT-001",
  "context": {
    "currency": "XAF",
    "amount": 50000
  }
}
```

**Response:**

```json
{
  "id": "flw_dep123",
  "flowType": "FIRST_DEPOSIT",
  "status": "RUNNING",
  "currentStep": "USER_SUBMITS",
  "stepIds": [
    "stp_submit",
    "stp_approve",
    "stp_complete"
  ],
  "context": {
    "currency": "XAF",
    "amount": 50000
  }
}
```

### Step 3: Submit Deposit Details

```http
POST /bff/steps/stp_submit HTTP/1.1
Content-Type: application/json

{
  "input": {
    "receiptUrl": "https://storage.example.com/receipts/abc.pdf",
    "paymentMethod": "MOBILE_MONEY",
    "transactionRef": "TXN123456"
  }
}
```

**Response:**

```json
{
  "id": "stp_submit",
  "stepType": "USER_SUBMITS",
  "status": "COMPLETED",
  "output": {
    "submittedAt": "2024-03-15T11:00:00Z",
    "waitingForApproval": true
  }
}
```

### Step 4: Poll for Approval

```http
GET /bff/flows/flw_dep123 HTTP/1.1
```

**Response (Pending):**

```json
{
  "flow": {
    "status": "RUNNING",
    "currentStep": "ADMIN_APPROVES"
  },
  "steps": [
    {
      "id": "stp_submit",
      "status": "COMPLETED"
    },
    {
      "id": "stp_approve",
      "status": "PENDING_EXTERNAL"
    }
  ]
}
```

**Response (Approved):**

```json
{
  "flow": {
    "status": "COMPLETED",
    "currentStep": null
  },
  "steps": [
    {
      "id": "stp_submit",
      "status": "COMPLETED"
    },
    {
      "id": "stp_approve",
      "status": "COMPLETED",
      "actor": "STAFF"
    },
    {
      "id": "stp_complete",
      "status": "COMPLETED"
    }
  ]
}
```

---

## KYC Level

### Purpose
Query the user's KYC verification status based on completed flows.

### Endpoint

```http
GET /bff/users/{userId}/kyc-level HTTP/1.1
```

### Response

```json
{
  "userId": "usr_abc123",
  "level": ["NONE", "PHONE_OTP_VERIFIED", "FIRST_DEPOSIT_VERIFIED"],
  "phoneOtpVerified": true,
  "firstDepositVerified": true
}
```

### KYC Levels

| Level | Meaning |
|-------|---------|
| `NONE` | No KYC completed (always present as baseline) |
| `PHONE_OTP_VERIFIED` | Phone number verified via OTP |
| `FIRST_DEPOSIT_VERIFIED` | First deposit approved |

### Typical Flow

1. Start: `["NONE"]` - No KYC completed
2. After Phone OTP: `["NONE", "PHONE_OTP_VERIFIED"]`
3. After First Deposit: `["NONE", "PHONE_OTP_VERIFIED", "FIRST_DEPOSIT_VERIFIED"]`

### Quick Check

```javascript
const response = await fetch('/bff/users/usr_abc123/kyc-level', {
  headers: { /* signature headers */ }
});
const { phoneOtpVerified, firstDepositVerified } = await response.json();

if (phoneOtpVerified && firstDepositVerified) {
  console.log('User has completed full KYC');
}
```

---

## Error Handling

### Common Errors

| Status | Code                | Description                   |
|--------|---------------------|-------------------------------|
| 401    | `MISSING_SIGNATURE` | Auth headers missing          |
| 401    | `INVALID_SIGNATURE` | Signature verification failed |
| 401    | `REPLAY_DETECTED`   | Nonce already used            |
| 403    | `DEVICE_INACTIVE`   | Device not active             |
| 404    | `FLOW_NOT_FOUND`    | Invalid flow ID               |
| 400    | `INVALID_OTP`       | Wrong OTP code                |
| 400    | `OTP_EXPIRED`       | OTP expired (5 min)           |
| 400    | `MAX_ATTEMPTS`      | Too many retry attempts       |

### Error Response Format

```json
{
  "error": {
    "code": "INVALID_OTP",
    "message": "OTP code is invalid or expired",
    "details": {
      "attemptsRemaining": 2
    }
  }
}
```

---

## Step Status Values

| Status             | Meaning                     |
|--------------------|-----------------------------|
| `PENDING_INPUT`    | Waiting for user input      |
| `PENDING_EXTERNAL` | Waiting for external system |
| `RUNNING`          | Processing in worker        |
| `COMPLETED`        | Step done successfully      |
| `FAILED`           | Step failed (check error)   |
| `CANCELLED`        | Step cancelled              |

---

## Actor Types

| Actor      | Description                        |
|------------|------------------------------------|
| `USER`     | End user action                    |
| `STAFF`    | Admin/staff action (via Staff API) |
| `SYSTEM`   | Automated system action            |
| `EXTERNAL` | External webhook response          |

---

## Integration Checklist

- [ ] Generate P-256 key pair on device enrollment
- [ ] Store private key securely (Keychain/Keystore)
- [ ] Register public JWK with backend
- [ ] Implement canonical payload builder
- [ ] Implement ES256 signer
- [ ] Implement nonce generator (UUID recommended)
- [ ] Handle 401 errors (re-enroll device)
- [ ] Handle 400 errors (retry with new input)
- [ ] Poll flow status for async steps