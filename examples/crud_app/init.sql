CREATE TABLE IF NOT EXISTS todos (
  id integer PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
  title varchar(255) NOT NULL,
  description text,
  completed boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);