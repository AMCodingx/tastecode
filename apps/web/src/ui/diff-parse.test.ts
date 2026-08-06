import { describe, expect, it } from 'vitest'
import { parseDiff } from './Diff.js'

const SAMPLE = `diff --git a/src/app.ts b/src/app.ts
index 1a2b3c4..5d6e7f8 100644
--- a/src/app.ts
+++ b/src/app.ts
@@ -1,4 +1,5 @@
 import { start } from './server.js'
-start(3000)
+start(4311)
+console.log('up')
 `

describe('diff parsing', () => {
  it('counts additions and deletions without miscounting file headers', () => {
    // The trap: +++ and --- start with + and -, so a naive prefix check
    // reports every file as one extra addition and one extra deletion.
    const parsed = parseDiff(SAMPLE)
    expect(parsed.added).toBe(2)
    expect(parsed.removed).toBe(1)
    expect(parsed.files).toBe(1)
    expect(parsed.fileEntries).toEqual([{ path: 'src/app.ts', added: 2, removed: 1 }])
  })

  it('classifies each line', () => {
    const kinds = parseDiff(SAMPLE).lines.map((line) => line.kind)
    expect(kinds).toContain('meta')
    expect(kinds).toContain('hunk')
    expect(kinds).toContain('add')
    expect(kinds).toContain('del')
    expect(kinds).toContain('ctx')
  })

  it('does not mistake content that merely starts with -- or ++ for a header', () => {
    // A deleted SQL comment and an added pre-increment. Git headers always
    // carry a trailing space; these do not, and they are real changes.
    const tricky = `diff --git a/db.sql b/db.sql
--- a/db.sql
+++ b/db.sql
@@ -1,2 +1,2 @@
--- reset the sequence
+++i;
 SELECT 1`
    const parsed = parseDiff(tricky)
    expect(parsed.added).toBe(1)
    expect(parsed.removed).toBe(1)
    expect(parsed.fileEntries).toEqual([{ path: 'db.sql', added: 1, removed: 1 }])
  })

  it('counts multiple files', () => {
    const two = `${SAMPLE}\ndiff --git a/b.ts b/b.ts\n+x`
    const parsed = parseDiff(two)
    expect(parsed.files).toBe(2)
    expect(parsed.fileEntries[1]).toEqual({ path: 'b.ts', added: 1, removed: 0 })
  })
})
