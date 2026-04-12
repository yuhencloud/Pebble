const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const targetTriple = execSync('rustc -vV').toString().match(/host:\s*(.+)/)[1].trim();
const ext = process.platform === 'win32' ? '.exe' : '';

const explicitProfile = process.argv[2] === '--debug' ? 'debug' : (process.argv[2] === '--release' ? 'release' : null);

let src;
if (explicitProfile) {
  src = path.resolve(__dirname, '..', 'target', explicitProfile, `pebble-bridge${ext}`);
} else {
  // Auto-detect: prefer release, fallback to debug
  const releaseSrc = path.resolve(__dirname, '..', 'target', 'release', `pebble-bridge${ext}`);
  const debugSrc = path.resolve(__dirname, '..', 'target', 'debug', `pebble-bridge${ext}`);
  src = fs.existsSync(releaseSrc) ? releaseSrc : debugSrc;
}

const destDir = path.resolve(__dirname, '..', 'bin');
const dest = path.join(destDir, `pebble-bridge-${targetTriple}${ext}`);

if (!fs.existsSync(src)) {
  console.error(`Bridge binary not found: ${src}`);
  console.error('Run: cargo build --release --bin pebble-bridge');
  process.exit(1);
}

fs.mkdirSync(destDir, { recursive: true });
fs.copyFileSync(src, dest);
console.log(`Copied bridge binary to ${dest}`);
