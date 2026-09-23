ALTER TABLE agent_runs ADD COLUMN context_usage jsonb
  CHECK (context_usage IS NULL OR jsonb_typeof(context_usage) = 'object');
