ALTER TABLE agent_runs ADD COLUMN compactions jsonb NOT NULL DEFAULT '[]'
  CHECK (jsonb_typeof(compactions) = 'array');
