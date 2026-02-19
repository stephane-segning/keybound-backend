CREATE TABLE app_user (
  user_id text PRIMARY KEY,
  realm text NOT NULL,
  username text NOT NULL,
  first_name text,
  last_name text,
  email text,
  email_verified boolean NOT NULL DEFAULT false,
  phone_number text,
  fineract_customer_id text,
  disabled boolean NOT NULL DEFAULT false,
  attributes jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_user_phone ON app_user(phone_number);
CREATE INDEX idx_user_email ON app_user(email);
CREATE INDEX idx_user_fineract ON app_user(fineract_customer_id);
