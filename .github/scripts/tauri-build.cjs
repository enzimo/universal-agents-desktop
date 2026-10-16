const { spawnSync } = require('node:child_process');
const result = spawnSync(process.platform === 'win32' ? 'npx.cmd' : 'npx',
  ['--yes', '@tauri-apps/cli@2.11.4', 'build', ...process.argv.slice(2)],
  { stdio: 'inherit', shell: process.platform === 'win32' });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
