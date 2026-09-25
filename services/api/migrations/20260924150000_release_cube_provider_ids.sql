-- Released operations retain their audit row but must not reserve a provider
-- sandbox id. Cube providers may reuse ids after deletion.
UPDATE case_sandboxes
SET sandbox_id = NULL
WHERE state = 'released' AND sandbox_id IS NOT NULL;
