ALTER TABLE agent_tool_permissions DROP CONSTRAINT agent_tool_permissions_tool_check;
ALTER TABLE agent_tool_permissions ADD CONSTRAINT agent_tool_permissions_tool_check
  CHECK(tool IN ('memory_save','case_set_context','proposal_create','request_proposal_approval'));
ALTER TABLE agent_tool_permissions ADD COLUMN proposal_id uuid REFERENCES proposals(id);
ALTER TABLE agent_tool_permissions ADD CONSTRAINT business_permission_proposal
  CHECK((tool='request_proposal_approval') = (proposal_id IS NOT NULL));
