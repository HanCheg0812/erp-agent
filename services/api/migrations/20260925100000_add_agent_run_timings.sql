ALTER TABLE agent_runs
  ADD COLUMN timings jsonb NOT NULL DEFAULT '[]'::jsonb
  CHECK (jsonb_typeof(timings) = 'array');
