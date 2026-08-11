#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')

function readJson(path) {
  return JSON.parse(readFileSync(join(root, path), 'utf8'))
}

function readCargoVersion(path) {
  const source = readFileSync(join(root, path), 'utf8')
  const match = source.match(/^version\s*=\s*"([^"]+)"/m)
  if (!match) throw new Error(`无法从 ${path} 读取版本号`)
  return match[1]
}

const versions = new Map([
  ['Cargo.toml', readCargoVersion('Cargo.toml')],
  ['desktop/src-tauri/Cargo.toml', readCargoVersion('desktop/src-tauri/Cargo.toml')],
  ['frontend/package.json', readJson('frontend/package.json').version],
  ['frontend/package-lock.json', readJson('frontend/package-lock.json').version],
  ['desktop/package.json', readJson('desktop/package.json').version],
  ['desktop/package-lock.json', readJson('desktop/package-lock.json').version],
  ['desktop/src-tauri/tauri.conf.json', readJson('desktop/src-tauri/tauri.conf.json').version],
])

const uniqueVersions = new Set(versions.values())
if (uniqueVersions.size !== 1 || [...versions.values()].some((value) => !value)) {
  const details = [...versions.entries()].map(([file, version]) => `${file}=${version || '<empty>'}`).join(', ')
  throw new Error(`项目版本号不一致：${details}`)
}

const version = [...uniqueVersions][0]
const refName = process.env.RELEASE_VERSION || ''
if (refName.startsWith('v') && refName !== `v${version}`) {
  throw new Error(`版本标签 ${refName} 与项目版本 ${version} 不一致`)
}

console.log(`Release metadata is consistent: v${version}`)
