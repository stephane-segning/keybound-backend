-- Down migration for the revamp.
-- This intentionally drops the new tables only (legacy schema is not recreated).

DROP TABLE IF EXISTS sm_step_attempt CASCADE;
DROP TABLE IF EXISTS sm_event CASCADE;
DROP TABLE IF EXISTS sm_instance CASCADE;

DROP TABLE IF EXISTS device CASCADE;
DROP TABLE IF EXISTS app_user CASCADE;

