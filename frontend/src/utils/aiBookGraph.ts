import type { AiBookMemory } from '../types'

export interface AiBookGraphNode {
  id: string
  label: string
  kind: 'character' | 'location'
  detail?: string
}

export interface AiBookGraphLink {
  source: string
  target: string
  label: string
}

export interface AiBookRelationshipGraph {
  nodes: AiBookGraphNode[]
  links: AiBookGraphLink[]
}

export interface AiBookGraphLayoutNode extends AiBookGraphNode {
  x: number
  y: number
  width: number
  height: number
  lane: 'left' | 'right' | 'center'
  dimmed: boolean
  connectedToSelected: boolean
}

export interface AiBookGraphLayoutLink extends AiBookGraphLink {
  path: string
  labelX: number
  labelY: number
  dimmed: boolean
  highlighted: boolean
  showLabel: boolean
}

export interface AiBookGraphLayout {
  width: number
  height: number
  nodes: AiBookGraphLayoutNode[]
  links: AiBookGraphLayoutLink[]
}

export function buildAiBookRelationshipGraph(memory: AiBookMemory): AiBookRelationshipGraph {
  const nodes: AiBookGraphNode[] = []
  const links: AiBookGraphLink[] = []
  const seenNodes = new Set<string>()
  const seenLinks = new Set<string>()

  function addNode(node: AiBookGraphNode) {
    if (!node.id || seenNodes.has(node.id)) return
    seenNodes.add(node.id)
    nodes.push(node)
  }

  function addLink(link: AiBookGraphLink) {
    if (!link.source || !link.target || link.source === link.target) return
    const key = `${link.source}\u0000${link.target}\u0000${link.label}`
    if (seenLinks.has(key)) return
    seenLinks.add(key)
    links.push(link)
  }

  for (const character of memory.characters) {
    addNode({
      id: character.name,
      label: character.name,
      kind: 'character',
      detail: character.status || character.description,
    })
  }

  for (const relationship of memory.relationships) {
    addNode({
      id: relationship.source,
      label: relationship.source,
      kind: 'character',
    })
    addNode({
      id: relationship.target,
      label: relationship.target,
      kind: 'character',
    })
    addLink({
      source: relationship.source,
      target: relationship.target,
      label: relationship.relation || relationship.status || '关联',
    })
  }

  for (const location of memory.locations) {
    addNode({
      id: location.name,
      label: location.name,
      kind: 'location',
      detail: location.description,
    })
    for (const characterName of location.relatedCharacters || []) {
      addNode({
        id: characterName,
        label: characterName,
        kind: 'character',
      })
      addLink({
        source: characterName,
        target: location.name,
        label: '位于',
      })
    }
  }

  return { nodes, links }
}

export function layoutAiBookRelationshipGraph(
  graph: AiBookRelationshipGraph,
  selectedId = '',
): AiBookGraphLayout {
  const width = 920
  const nodeMap = new Map(graph.nodes.map((node) => [node.id, node]))
  const degreeMap = new Map<string, number>()
  for (const link of graph.links) {
    degreeMap.set(link.source, (degreeMap.get(link.source) || 0) + 1)
    degreeMap.set(link.target, (degreeMap.get(link.target) || 0) + 1)
  }

  const activeSelectedId = selectedId && nodeMap.has(selectedId) ? selectedId : ''
  const connectedIds = new Set<string>()
  if (activeSelectedId) {
    connectedIds.add(activeSelectedId)
    for (const link of graph.links) {
      if (link.source === activeSelectedId) connectedIds.add(link.target)
      if (link.target === activeSelectedId) connectedIds.add(link.source)
    }
  }

  const locations = sortNodesForLayout(graph.nodes.filter((node) => node.kind === 'location'), degreeMap)
  const characters = sortNodesForLayout(graph.nodes.filter((node) => node.kind === 'character'), degreeMap)
  const densestLaneCount = Math.max(1, locations.length, characters.length)
  const height = Math.max(520, densestLaneCount * 56 + 118)
  const allLocationsOnly = locations.length > 0 && characters.length === 0
  const allCharactersOnly = characters.length > 0 && locations.length === 0

  const positioned = new Map<string, AiBookGraphLayoutNode>()
  const leftX = allCharactersOnly ? width * 0.34 : width * 0.26
  const rightX = allLocationsOnly ? width * 0.66 : width * 0.72
  placeColumn(locations, allLocationsOnly ? 'center' : 'left', allLocationsOnly ? width / 2 : leftX, height, positioned, degreeMap, connectedIds, activeSelectedId)
  placeColumn(characters, allCharactersOnly ? 'center' : 'right', allCharactersOnly ? width / 2 : rightX, height, positioned, degreeMap, connectedIds, activeSelectedId)

  const links = graph.links.flatMap((link): AiBookGraphLayoutLink[] => {
    const source = positioned.get(link.source)
    const target = positioned.get(link.target)
    if (!source || !target) return []
    const highlighted = Boolean(activeSelectedId && (link.source === activeSelectedId || link.target === activeSelectedId))
    const dimmed = Boolean(activeSelectedId && !highlighted)
    const { path, labelX, labelY } = buildLinkPath(source, target)
    return [{
      ...link,
      path,
      labelX,
      labelY,
      highlighted,
      dimmed,
      showLabel: highlighted || (graph.links.length <= 10 && link.label !== '位于'),
    }]
  })

  return {
    width,
    height,
    nodes: graph.nodes.flatMap((node) => positioned.get(node.id) || []),
    links,
  }
}

function sortNodesForLayout(nodes: AiBookGraphNode[], degreeMap: Map<string, number>) {
  return nodes.slice().sort((a, b) => {
    const degreeDelta = (degreeMap.get(b.id) || 0) - (degreeMap.get(a.id) || 0)
    if (degreeDelta !== 0) return degreeDelta
    return a.label.localeCompare(b.label, 'zh-Hans-CN')
  })
}

function placeColumn(
  nodes: AiBookGraphNode[],
  lane: AiBookGraphLayoutNode['lane'],
  centerX: number,
  layoutHeight: number,
  positioned: Map<string, AiBookGraphLayoutNode>,
  degreeMap: Map<string, number>,
  connectedIds: Set<string>,
  selectedId: string,
) {
  const top = 62
  const bottom = layoutHeight - 62
  const count = Math.max(1, nodes.length)
  const step = count === 1 ? 0 : (bottom - top) / (count - 1)
  const centerY = (top + bottom) / 2

  nodes.forEach((node, index) => {
    const width = node.kind === 'location'
      ? clamp(node.label.length * 15 + 42, 126, 192)
      : clamp(node.label.length * 14 + 36, 104, 176)
    const height = selectedId === node.id ? 48 : 42
    const y = count === 1 ? centerY : Math.round(top + step * index)
    const dimmed = Boolean(selectedId && !connectedIds.has(node.id))
    positioned.set(node.id, {
      ...node,
      x: Math.round(centerX - width / 2),
      y: Math.round(y - height / 2),
      width,
      height,
      lane,
      dimmed,
      connectedToSelected: Boolean(selectedId && connectedIds.has(node.id) && selectedId !== node.id),
      detail: node.detail || `关联 ${degreeMap.get(node.id) || 0} 条线索`,
    })
  })
}

function buildLinkPath(source: AiBookGraphLayoutNode, target: AiBookGraphLayoutNode) {
  const sourceX = source.x + source.width / 2
  const sourceY = source.y + source.height / 2
  const targetX = target.x + target.width / 2
  const targetY = target.y + target.height / 2
  const leftToRight = sourceX <= targetX
  const startX = leftToRight ? source.x + source.width : source.x
  const endX = leftToRight ? target.x : target.x + target.width
  const startY = sourceY
  const endY = targetY
  const sameLane = source.lane === target.lane
  const bend = sameLane
    ? (source.lane === 'left' ? -86 : 86)
    : Math.max(96, Math.abs(endX - startX) * 0.42)
  const c1x = sameLane ? startX + bend : startX + (leftToRight ? bend : -bend)
  const c2x = sameLane ? endX + bend : endX + (leftToRight ? -bend : bend)
  const labelX = Math.round((sourceX + targetX) / 2)
  const labelY = Math.round((sourceY + targetY) / 2) - 8

  return {
    path: `M ${Math.round(startX)} ${Math.round(startY)} C ${Math.round(c1x)} ${Math.round(startY)}, ${Math.round(c2x)} ${Math.round(endY)}, ${Math.round(endX)} ${Math.round(endY)}`,
    labelX,
    labelY,
  }
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value))
}
