#!/usr/bin/env node
// Builds the Windows desktop packages:
//   frontend -> release binary -> NSIS installer + portable zip
//
// The frontend assets are embedded into the exe at compile time by
// `tauri::generate_context!` (frontendDist), so the portable folder is just the
// executable:
//   Reader.exe
//   data/          <- created on first launch, next to the exe
//
// Outputs (in desktop/dist/):
//   Reader_<ver>_x64-setup.exe   NSIS installer
//   reader-portable-v<ver>-win-x64.zip
//
// NOTE: cargo release builds must be low-concurrency (`-j 4`) because Windows
// antivirus real-time scanning races cargo's parallel .rlib writes and causes
// random "invalid metadata" / link.exe failures. We set CARGO_BUILD_JOBS=4 so
// the `tauri build` step inherits it.

import { execFileSync, execSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const frontendDir = join(repoRoot, 'frontend')
const desktopDir = join(repoRoot, 'desktop')
const tauriDir = join(desktopDir, 'src-tauri')
const outRoot = join(desktopDir, 'dist')
const stageDir = join(outRoot, 'Reader')
// tauri build resolves beforeBuildCommand's relative path (../../frontend)
// against the CWD, so it must run from src-tauri/ (where ../../frontend is
// correct). Invoke the CLI by absolute path to avoid npx re-resolving.
const tauriCli = join(desktopDir, 'node_modules', '@tauri-apps', 'cli', 'tauri.js')

function run(command, args, cwd = repoRoot, env = {}) {
  console.log(`\n> ${command} ${args.join(' ')}`)
  execFileSync(command, args, { cwd, stdio: 'inherit', env: { ...process.env, ...env } })
}

/// For commands that need a shell to resolve (npm is npm.cmd on Windows).
function runShell(command, cwd = repoRoot, env = {}) {
  console.log(`\n> ${command}`)
  execSync(command, { cwd, stdio: 'inherit', env: { ...process.env, ...env } })
}

function readVersion() {
  const cargoToml = readFileSync(join(repoRoot, 'Cargo.toml'), 'utf8')
  const match = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)
  if (!match) throw new Error('无法从 Cargo.toml 读取版本号')
  return match[1]
}

const version = readVersion()
console.log(`打包 阅读 桌面版 v${version}`)

// `ci` rather than `install` so a release build can never drift the lockfile.
runShell('npm ci', frontendDir)
// Build the frontend here (beforeBuildCommand is a no-op) so the relative
// path in tauri.conf.json never has to resolve against tauri's CWD.
runShell('npm run build', frontendDir)

// `tauri build` runs cargo release build + NSIS bundling. CARGO_BUILD_JOBS=4
// keeps the antivirus race at bay. Run from src-tauri/ so the relative
// frontendDist path resolves correctly.
run('node', [tauriCli, 'build'], tauriDir, { CARGO_BUILD_JOBS: '4' })

// ── NSIS installer ──
// The workspace shares the repo-root target/ dir, so the bundle lands under
// <repoRoot>/target/release/bundle/nsis, not desktop/src-tauri/target.
const nsisDir = join(repoRoot, 'target', 'release', 'bundle', 'nsis')
const nsisSrc = join(nsisDir, `Reader_${version}_x64-setup.exe`)
if (!existsSync(nsisSrc)) {
  throw new Error(`未找到 NSIS 安装包: ${nsisSrc}`)
}
const nsisOut = join(outRoot, `Reader_${version}_x64-setup.exe`)
rmSync(nsisOut, { force: true })
cpSync(nsisSrc, nsisOut)
console.log(`\nNSIS 安装包: ${nsisOut}`)

// ── Portable zip ──
const exeSrc = join(repoRoot, 'target', 'release', 'reader-desktop.exe')
if (!existsSync(exeSrc)) {
  throw new Error(`未找到编译产物: ${exeSrc}`)
}

rmSync(stageDir, { recursive: true, force: true })
mkdirSync(stageDir, { recursive: true })
cpSync(exeSrc, join(stageDir, 'Reader.exe'))
writeFileSync(join(stageDir, '.reader-portable'), 'Reader portable package\n')
// data/ is created at first launch — never ship it in the zip.
rmSync(join(stageDir, 'data'), { recursive: true, force: true })

const zipPath = join(outRoot, `reader-portable-v${version}-win-x64.zip`)
rmSync(zipPath, { force: true })
run('tar.exe', ['-a', '-c', '-f', zipPath, '-C', stageDir, '.'])

console.log(`\n完成:\n  安装包 ${nsisOut}\n  压缩包 ${zipPath}`)
