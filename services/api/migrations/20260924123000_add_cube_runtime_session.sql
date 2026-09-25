ALTER TABLE case_sandboxes
  ADD COLUMN mcp_session_id text REFERENCES mcp_sessions(id);

CREATE INDEX case_sandboxes_mcp_session_idx ON case_sandboxes(mcp_session_id)
  WHERE mcp_session_id IS NOT NULL;
