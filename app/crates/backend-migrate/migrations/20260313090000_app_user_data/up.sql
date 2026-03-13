CREATE TABLE app_user_data (
  user_id text NOT NULL REFERENCES app_user(user_id) ON DELETE CASCADE,
  name text NOT NULL,
  data_type text NOT NULL,
  content jsonb NOT NULL DEFAULT '{}'::jsonb,
  eager_fetch boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, name, data_type)
);

CREATE INDEX app_user_data_user_id_idx ON app_user_data(user_id);
CREATE INDEX app_user_data_eager_fetch_idx ON app_user_data(user_id, eager_fetch);
