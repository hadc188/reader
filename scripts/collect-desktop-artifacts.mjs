#!/usr/bin/env node

import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
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

function copyArtifact(source, destination) {
  if (!existsSync(source) || !statSync(source).isFile() || statSync(source).size === 0) {
    throw new Error(`Bundle is missing or empty: ${source}`)
  }
  cpSync(source, destination)
  if (!existsSync(destination) || statSync(destination).size === 0) {
    throw new Error(`Failed to copy bundle: ${destination}`)
  }
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

function findAppBundle() {
  return bundleRoots()
    .flatMap((rootPath) => findEntries(rootPath, (path) => path.toLowerCase().endsWith('.app')))
    .find(Boolean)
}

function zipAppBundle(app, destination) {
  execFileSync('ditto', [
    '-c', '-k', '--sequesterRsrc', '--keepParent', app,
    destination,
  ])
}

function zipAppFromDmg(dmg, destination) {
  const mountPoint = join(tmpdir(), `reader-dmg-${process.pid}`)
  rmSync(mountPoint, { recursive: true, force: true })
  mkdirSync(mountPoint, { recursive: true })
  let mounted = false

  try {
    execFileSync('hdiutil', [
      'attach', dmg,
      '-mountpoint', mountPoint,
      '-nobrowse',
      '-readonly',
      '-quiet',
    ])
    mounted = true
    const app = findEntries(mountPoint, (path) => path.toLowerCase().endsWith('.app')).find(Boolean)
    if (!app) throw new Error(`No .app bundle found inside ${dmg}`)
    zipAppBundle(app, destination)
  } finally {
    if (mounted) {
      try {
        execFileSync('hdiutil', ['detach', mountPoint, '-quiet'])
      } catch (error) {
        console.warn(`Unable to detach ${mountPoint}: ${error.message}`)
      }
    }
    rmSync(mountPoint, { recursive: true, force: true })
  }
}

const prefix = `Reader-${version}-${platform}-${arch}`

if (platform === 'windows') {
  copyArtifact(firstBundleFile('.exe'), join(outputDir, `${prefix}-setup.exe`))

  const stage = join(tmpdir(), `reader-portable-${process.pid}`)
  rmSync(stage, { recursive: true, force: true })
  mkdirSync(stage, { recursive: true })
  cpSync(firstExecutable(), join(stage, 'Reader.exe'))
  writeFileSync(join(stage, '.reader-portable'), 'Reader portable package\n')
  execFileSync('tar.exe', ['-a', '-c', '-f', join(outputDir, `${prefix}-portable.zip`), '-C', stage, '.'])
  rmSync(stage, { recursive: true, force: true })
} else if (platform === 'macos') {
  const dmg = firstBundleFile('.dmg')
  const dmgOutput = join(outputDir, `${prefix}.dmg`)
  const appOutput = join(outputDir, `${prefix}.zip`)
  copyArtifact(dmg, dmgOutput)
  const app = findAppBundle()
  if (app) {
    zipAppBundle(app, appOutput)
  } else {
    // A DMG-only Tauri build may remove its staging .app after bundling. Mount
    // the finished disk image so the portable app archive remains available.
    zipAppFromDmg(dmg, appOutput)
  }
  if (!existsSync(appOutput) || statSync(appOutput).size === 0) {
    throw new Error(`Failed to create macOS app archive: ${appOutput}`)
  }
} else if (platform === 'linux') {
  const appImage = firstBundleFile('.appimage')
  execFileSync('bash', [join(root, 'scripts', 'fix-linux-appimage.sh'), appImage], {
    stdio: 'inherit',
  })
  copyArtifact(appImage, join(outputDir, `${prefix}.AppImage`))
  copyArtifact(firstBundleFile('.deb'), join(outputDir, `${prefix}.deb`))
} else {
  throw new Error(`Unsupported release platform: ${platform}`)
}

console.log(`Collected ${platform} ${arch} artifacts in ${outputDir}`)
