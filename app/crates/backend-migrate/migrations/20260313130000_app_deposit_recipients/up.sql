CREATE TABLE app_deposit_recipients (
  provider TEXT NOT NULL,
  full_name TEXT NOT NULL,
  phone_number TEXT NOT NULL,
  phone_regex TEXT NOT NULL,
  currency TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (provider, currency)
);

CREATE INDEX app_deposit_recipients_currency_idx
  ON app_deposit_recipients(currency);
