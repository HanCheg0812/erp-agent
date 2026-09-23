ALTER TABLE cases
  ADD COLUMN kind text NOT NULL DEFAULT 'case' CHECK (kind IN ('case', 'conversation')),
  ADD COLUMN conversation_request_id uuid,
  ALTER COLUMN customer_code DROP NOT NULL,
  ALTER COLUMN part_number DROP NOT NULL,
  ALTER COLUMN revision DROP NOT NULL,
  ALTER COLUMN demand_type DROP NOT NULL,
  ADD CONSTRAINT cases_conversation_request_unique UNIQUE (tenant_id, owner_id, conversation_request_id),
  ADD CONSTRAINT cases_context_required CHECK (
    (kind = 'case' AND customer_code IS NOT NULL AND part_number IS NOT NULL AND revision IS NOT NULL AND demand_type IS NOT NULL)
    OR (kind = 'conversation' AND customer_code IS NULL AND part_number IS NULL AND revision IS NULL AND demand_type IS NULL AND quantity_basis = 'unknown' AND status = 'analyzing')
  );
