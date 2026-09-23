import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, writeFile, readFile, chmod } from 'node:fs/promises';
import pg from 'pg';

// This bootstrap is restricted to the local synthetic ERPNext demo.
const run = promisify(execFile);
const code = `
import frappe, json, secrets
from frappe.utils.password import set_encrypted_password
role = 'Forge Read Only'
email = 'forge-reader@example.test'
if not frappe.db.exists('Role', role):
    frappe.get_doc({'doctype':'Role','role_name':role,'desk_access':0}).insert(ignore_permissions=True)
for doctype in ['Sales Order', 'Customer']:
    if not frappe.db.exists('Custom DocPerm', {'parent':doctype,'role':role}):
        frappe.get_doc({'doctype':'Custom DocPerm','parent':doctype,'role':role,'permlevel':0,'read':1,'select':1}).insert(ignore_permissions=True)
if not frappe.db.exists('User', email):
    frappe.get_doc({'doctype':'User','email':email,'first_name':'Forge Reader','enabled':1,'send_welcome_email':0,'user_type':'System User','roles':[{'role':role}]}).insert(ignore_permissions=True)
user = frappe.get_doc('User', email)
user.set('roles', [{'role':role}])
user.api_key = secrets.token_hex(12)
user.save(ignore_permissions=True)
secret = secrets.token_hex(24)
set_encrypted_password('User', email, secret, 'api_secret')
customers = frappe.get_all('Customer', pluck='name')
frappe.db.commit()
frappe.clear_cache()
print(json.dumps({'token':user.api_key+':'+secret,'customers':customers}))
`;
const db = new pg.Client({ connectionString: process.env.DATABASE_URL }); await db.connect();
try {
  const existing = await readFile('.runtime/integrations.env', 'utf8').catch(error => { if (error.code === 'ENOENT') return ''; throw error; });
  const users = await db.query('SELECT id,tenant_id FROM users WHERE email=$1 AND active', [process.env.DEMO_EMAIL]);
  if (users.rows.length !== 1) throw new Error('Exactly one dedicated demo account is required');
  const { stdout } = await run('docker', ['exec','garment-demo-backend-1','bench','--site','frontend','execute',"__import__('builtins').exec",'--args',JSON.stringify([code])], { maxBuffer: 1024 * 1024 }).catch(error => { throw new Error(`ERP demo bootstrap failed (exit ${error.code}); integration credentials were not printed`); });
  const result = JSON.parse(stdout.trim().split('\n').find(line => line.startsWith('{"token"')));
  const user = users.rows[0];
  await db.query('BEGIN');
  for (const customer of result.customers) await db.query("INSERT INTO integration_grants(tenant_id,owner_id,domain,scope) VALUES($1,$2,'erp',$3) ON CONFLICT DO NOTHING", [user.tenant_id,user.id,customer]);
  await db.query('COMMIT');
  await mkdir('.runtime', { recursive:true, mode:0o700 });
  const retained = existing.split('\n').filter(line => !/^ERP_API_(URL|TOKEN)=/.test(line)).join('\n');
  await writeFile('.runtime/integrations.env', `${retained}\nERP_API_URL=http://127.0.0.1:18080\nERP_API_TOKEN=${result.token}\n`, { mode:0o600 });
  await chmod('.runtime/integrations.env', 0o600);
  console.log(JSON.stringify({ configured:true, erp_role:'Forge Read Only', granted_customers:result.customers.length, credentials:'.runtime/integrations.env' }));
} finally { await db.end(); }
