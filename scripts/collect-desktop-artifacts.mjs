#!/usr/bin/env node

import { cpSync, existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { join, resolve } from 'node:path'
import { tmpdir } from 'node:os'

const root = resolve(import.meta.dirname, '..')
const target = process.env.TAURI_TARGET
const version = process.env.RELEASE_VERSION || 'dev'
const platform = process.env.RELEASE_PLATFORM
const arch = process.env.RELEASE_ARCH

if (!target || !platform || !arch) {
  throw new Error('TAURI_TARGET, RELEASE_PLATFORM and RELEASE_ARCH are required')
}

const outputDir = join(root, 'release')
mkdirSync(outputDir, { recursive: true })

function findEntries(dir, predicate, result = []) {
  if (!existsSync(dir)) return result
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name)
    if (predicate(path)) {
      result.push(path)
      continue
    }
    if (entry.isDirectory()) {
      findEntries(path, predicate, result)
    }
  }
  return result
}

function findFilesWithExtension(dir, extension) {
  return findEntries(dir, (path) => path.toLowerCase().endsWith(extension))
}

function bundleRoots() {
  return [
    join(root, 'target', target, 'release', 'bundle'),
    join(root, 'target', 'release', 'bundle'),
  ]
}

function firstBundleFile(extension) {
  for (const rootPath of bundleRoots()) {
    const match = findFilesWithExtension(rootPath, extension)[0]
    if (match) return match
  }
  throw new Error(`No ${extension} bundle found for ${target}`)
}

function firstExecutable() {
  const candidates = [
    join(root, 'target', target, 'release', 'reader-desktop.exe'),
    join(root, 'target', 'release', 'reader-desktop.exe'),
  ]
  const match = candidates.find(existsSync)
  if (!match) throw new Error(`No Windows executable found for ${target}`)
  return match
}

const prefix = `Reader-${version}-${platform}-${arch}`

if (platform === 'windows') {
  cpSync(firstBundleFile('.exe'), join(outputDir, `${prefix}-setup.exe`))

  const stage = join(tmpdir(), `reader-portable-${process.pid}`)
  rmSync(stage, { recursive: true, force: true })
  mkdirSync(stage, { recursive: true })
  cpSync(firstExecutable(), join(stage, 'Reader.exe'))
  execFileSync('tar.exe', ['-a', '-c', '-f', join(outputDir, `${prefix}-portable.zip`), '-C', stage, 'Reader.exe'])
  rmSync(stage, { recursive: true, force: true })
} else if (platform === 'macos') {
  cpSync(firstBundleFile('.dmg'), join(outputDir, `${prefix}.dmg`))
  const app = bundleRoots()
    .flatMap((rootPath) => findEntries(rootPath, (path) => path.toLowerCase().endsWith('.app')))
    .find(Boolean)
  if (!app) throw new Error(`No .app bundle found for ${target}`)
  execFileSync('ditto', [
    '-c', '-k', '--sequesterRsrc', '--keepParent', app,
    join(outputDir, `${prefix}.zip`),
  ])
} else if (platform === 'linux') {
  cpSync(firstBundleFile('.appimage'), join(outputDir, `${prefix}.AppImage`))
  cpSync(firstBundleFile('.deb'), join(outputDir, `${prefix}.deb`))
} else {
  throw new Error(`Unsupported release platform: ${platform}`)
}

console.log(`Collected ${platform} ${arch} artifacts in ${outputDir}`)
