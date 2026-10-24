const { execSync } = require('child_process');

const RCEDIT = 'node_modules/rcedit/bin/rcedit-x64.exe';

const tasks = [
  ['target/release/openlocalserver.exe', 'src-tauri/icons/icon.ico'],
  ['target/release/ols-mcp.exe', 'crates/ols-mcp/icon.ico'],
  ['target/release/ols-helper.exe', 'crates/ols-helper/icon.ico'],
  ['target/release/olsc.exe', 'crates/ols-cli/icon.ico'],
];

for (const [exe, ico] of tasks) {
  console.log('Processing:', exe);
  try {
    execSync(`"${RCEDIT}" "${exe}" --set-icon "${ico}"`, {
      timeout: 30000,
      stdio: 'pipe',
      cwd: process.cwd(),
    });
    console.log('OK:', exe);
  } catch (e) {
    console.error('FAILED:', exe, e.message);
    if (e.stderr) console.error(e.stderr.toString());
    process.exit(1);
  }
}
console.log('ALL DONE');
