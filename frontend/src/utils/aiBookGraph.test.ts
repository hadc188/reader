import { describe, expect, it } from 'vitest'
import type { AiBookMemory } from '../types'
import { buildAiBookRelationshipGraph, layoutAiBookRelationshipGraph } from './aiBookGraph'

describe('aiBookGraph', () => {
  it('builds stable nodes and links from characters, relationships, and locations', () => {
    const memory: AiBookMemory = {
      bookUrl: 'book-1',
      enabled: true,
      updatedAt: 0,
      worldview: [],
      characters: [
        { name: '林舟', status: '受伤', location: '北境' },
        { name: '沈月', status: '失踪' },
      ],
      relationships: [
        { source: '林舟', target: '沈月', relation: '盟友' },
      ],
      locations: [
        { name: '北境', description: '寒冷边地', relatedCharacters: ['林舟'] },
      ],
    }

    const graph = buildAiBookRelationshipGraph(memory)

    expect(graph.nodes.map((node) => node.id)).toEqual(['林舟', '沈月', '北境'])
    expect(graph.links).toEqual([
      { source: '林舟', target: '沈月', label: '盟友' },
      { source: '林舟', target: '北境', label: '位于' },
    ])
  })

  it('lays out locations and characters in readable columns with selected-node focus', () => {
    const memory: AiBookMemory = {
      bookUrl: 'book-1',
      enabled: true,
      updatedAt: 0,
      worldview: [],
      characters: [
        { name: '林舟', status: '受伤', location: '北境' },
        { name: '沈月', status: '失踪' },
        { name: '韩青', status: '旁观' },
      ],
      relationships: [
        { source: '林舟', target: '沈月', relation: '盟友' },
      ],
      locations: [
        { name: '北境', description: '寒冷边地', relatedCharacters: ['林舟'] },
        { name: '帝都', description: '权力中心', relatedCharacters: ['韩青'] },
      ],
    }

    const graph = buildAiBookRelationshipGraph(memory)
    const layout = layoutAiBookRelationshipGraph(graph, '林舟')
    const north = layout.nodes.find((node) => node.id === '北境')!
    const lin = layout.nodes.find((node) => node.id === '林舟')!
    const han = layout.nodes.find((node) => node.id === '韩青')!
    const locatedLink = layout.links.find((link) => link.source === '林舟' && link.target === '北境')!
    const unrelatedLink = layout.links.find((link) => link.source === '韩青' && link.target === '帝都')!

    expect(north.lane).toBe('left')
    expect(lin.lane).toBe('right')
    expect(north.x).toBeLessThan(lin.x)
    expect(lin.dimmed).toBe(false)
    expect(north.dimmed).toBe(false)
    expect(han.dimmed).toBe(true)
    expect(locatedLink.highlighted).toBe(true)
    expect(locatedLink.showLabel).toBe(true)
    expect(unrelatedLink.dimmed).toBe(true)
    expect(unrelatedLink.showLabel).toBe(false)
  })

  it('expands the canvas for dense relationship graphs to avoid node overlap', () => {
    const memory: AiBookMemory = {
      bookUrl: 'book-1',
      enabled: true,
      updatedAt: 0,
      worldview: [],
      characters: Array.from({ length: 14 }, (_, index) => ({
        name: `角色${index + 1}`,
        status: '活跃',
      })),
      relationships: [],
      locations: [{
        name: '中心城',
        description: '主舞台',
        relatedCharacters: Array.from({ length: 14 }, (_, index) => `角色${index + 1}`),
      }],
    }

    const layout = layoutAiBookRelationshipGraph(buildAiBookRelationshipGraph(memory), '角色1')
    const characterNodes = layout.nodes
      .filter((node) => node.kind === 'character')
      .sort((a, b) => a.y - b.y)

    expect(layout.height).toBeGreaterThan(520)
    for (let index = 1; index < characterNodes.length; index += 1) {
      const previous = characterNodes[index - 1]!
      const current = characterNodes[index]!
      expect(current.y - previous.y).toBeGreaterThanOrEqual(46)
    }
  })
})
