import { describe, expect, it } from 'vitest'
import {
  MAX_SOURCE_TEST_BATCH_SIZE,
  chunkBookSourceUrls,
  mergeBookSourceTestResponses,
} from './sourceTesting'
import type { BookSourceTestResponse } from '../types'

describe('sourceTesting', () => {
  it('chunks source urls into frontend batches of at most 100', () => {
    const urls = Array.from({ length: 205 }, (_, index) => `https://source-${index}.example`)

    const chunks = chunkBookSourceUrls(urls)

    expect(MAX_SOURCE_TEST_BATCH_SIZE).toBe(100)
    expect(chunks).toHaveLength(3)
    expect(chunks.map((chunk) => chunk.length)).toEqual([100, 100, 5])
    expect(chunks.flat()).toEqual(urls)
  })

  it('merges source test responses across frontend batches', () => {
    const responses: BookSourceTestResponse[] = [
      {
        total: 2,
        valid: 1,
        invalid: 1,
        markedInvalid: 1,
        results: [
          sourceResult('https://valid.example', true),
          sourceResult('https://invalid.example', false),
        ],
      },
      {
        total: 1,
        valid: 1,
        invalid: 0,
        markedInvalid: 0,
        results: [sourceResult('https://valid-2.example', true)],
      },
    ]

    expect(mergeBookSourceTestResponses(responses)).toEqual({
      total: 3,
      valid: 2,
      invalid: 1,
      markedInvalid: 1,
      results: responses.flatMap((response) => response.results),
    })
  })
})

function sourceResult(bookSourceUrl: string, valid: boolean) {
  return {
    bookSourceName: bookSourceUrl,
    bookSourceUrl,
    valid,
    searchOk: valid,
    exploreOk: false,
    keyword: '斗破苍穹',
    markedInvalid: !valid,
  }
}
