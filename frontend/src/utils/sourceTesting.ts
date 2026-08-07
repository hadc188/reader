import type { BookSourceTestResponse } from '../types'

export const MAX_SOURCE_TEST_BATCH_SIZE = 100

export function chunkBookSourceUrls(
  urls: string[],
  batchSize = MAX_SOURCE_TEST_BATCH_SIZE
) {
  if (batchSize < 1) {
    throw new Error('batchSize must be greater than 0')
  }
  const chunks: string[][] = []
  for (let index = 0; index < urls.length; index += batchSize) {
    chunks.push(urls.slice(index, index + batchSize))
  }
  return chunks
}

export function mergeBookSourceTestResponses(
  responses: BookSourceTestResponse[]
): BookSourceTestResponse {
  return responses.reduce<BookSourceTestResponse>(
    (merged, response) => ({
      total: merged.total + response.total,
      valid: merged.valid + response.valid,
      invalid: merged.invalid + response.invalid,
      markedInvalid: merged.markedInvalid + response.markedInvalid,
      results: merged.results.concat(response.results),
    }),
    {
      total: 0,
      valid: 0,
      invalid: 0,
      markedInvalid: 0,
      results: [],
    }
  )
}
