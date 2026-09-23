ALTER TABLE case_conversation_messages DROP CONSTRAINT case_conversation_messages_text_check;
ALTER TABLE case_conversation_messages ADD CONSTRAINT case_conversation_messages_text_check CHECK (length(text) BETWEEN 0 AND 4000);
ALTER TABLE documents ADD CONSTRAINT documents_message_scope UNIQUE (tenant_id, owner_id, case_id, id);

CREATE TABLE conversation_message_attachments (
  tenant_id uuid NOT NULL,
  owner_id uuid NOT NULL,
  case_id uuid NOT NULL,
  message_id bigint NOT NULL,
  document_id uuid NOT NULL,
  ordinal smallint NOT NULL CHECK (ordinal BETWEEN 0 AND 3),
  source_sha256 text NOT NULL,
  PRIMARY KEY (message_id, ordinal),
  UNIQUE (message_id, document_id),
  FOREIGN KEY (tenant_id, case_id, message_id) REFERENCES case_conversation_messages(tenant_id, case_id, id),
  FOREIGN KEY (tenant_id, owner_id, case_id, document_id) REFERENCES documents(tenant_id, owner_id, case_id, id)
);
