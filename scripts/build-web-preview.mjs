import { cpSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const designRoot = join(root, 'opendesign');
const output = join(designRoot, 'mockups', 'forge-change-operations');
mkdirSync(output, { recursive: true });
cpSync(join(root, 'apps/web/dist'), output, { recursive: true });

function section(id, label) {
  const directory = join(designRoot, id);
  mkdirSync(directory, { recursive: true });
  const groups = readdirSync(directory, { withFileTypes: true }).filter(entry => entry.isDirectory()).map(entry => {
    const files = readdirSync(join(directory, entry.name), { recursive: true, withFileTypes: true })
      .filter(file => file.isFile() && (id !== 'mockups' || file.name.endsWith('.html')))
      .map(file => ({
        label: file.name,
        path: join(file.parentPath, file.name).slice(designRoot.length + 1).replaceAll('\\', '/'),
      }));
    return { slug: entry.name, files };
  }).filter(group => group.files.length);
  return { id, label, groups };
}

const manifest = { generated: new Date().toISOString(), sections: [section('mockups', 'Mockups'), section('design-systems', 'Design Systems')] };
writeFileSync(join(designRoot, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
console.log('OpenDesign preview rebuilt from the Web production bundle.');
