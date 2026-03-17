# Flow SDK runtime examples

These examples use the same YAML schema that the v2 runtime loads from `flows/*.yaml`.

## Runtime schema

```yaml
flow_type: phone_otp
human_id_prefix: phone_otp
initial_step: init_phone

steps:
  init_phone:
    action: WAIT
    actor: END_USER
    config:
      actor: USER
    next: get_user
```

Supported top-level fields:
- `flow_type`
- `human_id_prefix`
- `feature` (optional)
- `initial_step`
- `steps`

Supported step fields:
- `action`
- `actor`
- `config` (optional)
- `retry` (optional)
- `next` / `ok` / `fail`
- `branches` for named outcome routing

## Phone OTP example

`01_phone_otp.yaml` models:
- user phone input
- system user lookup
- OTP generation
- SMS webhook call
- end-user OTP verification
- metadata update via nested patch paths

## First deposit example

`02_first_deposit.yaml` models:
- end-user deposit init
- system user lookup
- admin approval step
- conditional approve/reject branching
- two CUSS webhooks
- metadata persistence
- clean reject closure

## Expected API flow

End user:
- `POST /bff/flow/sessions`
- `POST /bff/flow/sessions/{sessionId}/flows`
- `POST /bff/flow/steps/{stepId}`
- `GET /bff/flow/users/{userId}`

Staff:
- `GET /staff/flow/steps`
- `POST /staff/flow/steps/{stepId}`

## Notes

- `${VAR}` and `${VAR:-default}` are expanded when flow YAML files are loaded.
- `WEBHOOK_HTTP` templates support nested context lookups such as `{{flow.context.step_output.init_first_deposit.amount}}`.
- `branches` are the canonical way to route non-binary outcomes such as admin approve/reject.
