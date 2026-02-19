CREATE TABLE device (
  device_id text NOT NULL,
  user_id text NOT NULL REFERENCES app_user(user_id),
  jkt text NOT NULL,
  public_jwk text NOT NULL,
  status text NOT NULL,
  label text,
  created_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz,
  PRIMARY KEY (device_id, public_jwk)
);

CREATE UNIQUE INDEX device_device_id_unique ON device(device_id);
CREATE UNIQUE INDEX device_public_jwk_unique ON device(public_jwk);
CREATE INDEX device_user_id_idx ON device(user_id);
CREATE INDEX device_jkt_idx ON device(jkt);
