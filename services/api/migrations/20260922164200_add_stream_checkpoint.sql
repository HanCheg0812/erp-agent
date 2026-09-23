ALTER TABLE agent_session_documents DROP CONSTRAINT agent_session_documents_kind_check;
ALTER TABLE agent_session_documents ADD CONSTRAINT agent_session_documents_kind_check
  CHECK (kind IN ('todos','goals','plan','review','gate_observations','stream_checkpoint'));
