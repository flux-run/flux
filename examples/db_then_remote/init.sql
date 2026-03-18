CREATE TABLE IF NOT EXISTS outbound_dispatches (
  id integer PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
  order_id varchar(128) NOT NULL,
  message text NOT NULL,
  status varchar(32) NOT NULL DEFAULT 'pending',
  remote_status integer,
  created_at timestamptz NOT NULL DEFAULT now(),
  delivered_at timestamptz
);