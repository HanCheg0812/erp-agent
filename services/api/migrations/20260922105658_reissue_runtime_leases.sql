-- Reissue runtime leases after the API migration. Existing session data and
-- receipts remain intact; new runtimes load the current revision before writing.
UPDATE agent_sessions SET token_hash = NULL, expires_at = NULL;
