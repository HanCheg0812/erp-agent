ALTER TABLE agent_runs ADD COLUMN steps jsonb NOT NULL DEFAULT '[]'
  CHECK (jsonb_typeof(steps) = 'array');
