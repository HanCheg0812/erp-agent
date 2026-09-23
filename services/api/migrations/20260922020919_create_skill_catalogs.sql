CREATE TABLE agent_skill_catalogs (
  hash text PRIMARY KEY CHECK (hash ~ '^[a-f0-9]{64}$'),
  snapshot text NOT NULL CHECK (octet_length(snapshot) <= 65536),
  created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE agent_runs ADD COLUMN skill_catalog_hash text REFERENCES agent_skill_catalogs(hash);
ALTER TABLE agent_runs ADD COLUMN loaded_skills jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(loaded_skills) = 'array');
