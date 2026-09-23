CREATE INDEX agent_runs_dispatch_idx ON agent_runs (state, updated_at, message_id)
WHERE state IN ('queued', 'running', 'waiting');

CREATE FUNCTION notify_agent_run_queued() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  PERFORM pg_notify('erp_agent_runs', '');
  RETURN NEW;
END;
$$;

CREATE TRIGGER agent_run_queued_notify
AFTER INSERT OR UPDATE OF state ON agent_runs
FOR EACH ROW WHEN (NEW.state = 'queued')
EXECUTE FUNCTION notify_agent_run_queued();
