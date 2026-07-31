import type { Item } from '@harness/contracts'

/**
 * Turn boundaries within the flat item list.
 *
 * The thread stays one virtualised list — grouping into nested containers
 * would cost the flat index that virtualisation depends on. Instead we compute
 * where each turn starts and navigate by those indices.
 */

export type TurnMark = {
  turnId: string
  /** Index of the first item of the turn. */
  index: number
  /** How many items belong to it. */
  count: number
}

export type TurnPresentation = {
  activity: Item[]
  firstActivityIndex: number | undefined
  firstResponseIndex: number | undefined
  finalAnswerIndex: number | undefined
  elapsedMs: number
  complete: boolean
}

export function findTurns(items: Item[]): TurnMark[] {
  const turns: TurnMark[] = []

  items.forEach((item, index) => {
    // The optimistic user echo has no turn id yet; it still starts a turn
    // visually, so it gets its own boundary rather than joining the last one.
    const turnId = item.turnId === '' ? `local:${index}` : item.turnId
    const last = turns[turns.length - 1]
    if (last && last.turnId === turnId) {
      last.count += 1
      return
    }
    turns.push({ turnId, index, count: 1 })
  })

  return turns
}

/**
 * The compact, completed-turn view used by first-party agent apps.
 *
 * Commands and reasoning stay available, but they sit behind one elapsed-time
 * disclosure instead of interrupting the final answer as a transcript. The
 * indices let Thread keep one flat virtualised list while rendering that
 * disclosure only once.
 */
export function presentTurns(items: Item[]): ReadonlyMap<string, TurnPresentation> {
  const drafts = new Map<
    string,
    {
      activity: Item[]
      firstActivityIndex?: number
      firstResponseIndex?: number
      finalAnswerIndex?: number
      earliest: number
      latest: number
      hasRunningActivity: boolean
    }
  >()

  items.forEach((item, index) => {
    if (!item.turnId) return

    const draft = drafts.get(item.turnId) ?? {
      activity: [],
      earliest: item.createdAt,
      latest: item.createdAt,
      hasRunningActivity: false,
    }

    draft.earliest = Math.min(draft.earliest, item.createdAt)
    draft.latest = Math.max(draft.latest, item.createdAt)

    if (item.type !== 'message' || item.role !== 'user') {
      draft.firstResponseIndex ??= index
    }

    if (isActivity(item)) {
      draft.activity.push(item)
      draft.firstActivityIndex ??= index
      draft.hasRunningActivity ||= item.status === 'started'
    } else if (
      item.type === 'message' &&
      item.role === 'assistant' &&
      item.status === 'completed'
    ) {
      draft.finalAnswerIndex = index
    }

    drafts.set(item.turnId, draft)
  })

  return new Map(
    [...drafts].map(([turnId, draft]) => [
      turnId,
      {
        activity: draft.activity,
        firstActivityIndex: draft.firstActivityIndex,
        firstResponseIndex: draft.firstResponseIndex,
        finalAnswerIndex: draft.finalAnswerIndex,
        elapsedMs: Math.max(0, draft.latest - draft.earliest),
        complete: draft.finalAnswerIndex !== undefined && !draft.hasRunningActivity,
      },
    ]),
  )
}

function isActivity(item: Item): boolean {
  return item.type !== 'message' && item.type !== 'error'
}

/**
 * The turn to jump to from the current scroll position.
 *
 * Going back from inside a turn returns to that turn's own start first, which
 * is what "previous" means while reading — one press should not skip the thing
 * you are looking at.
 */
export function neighbourTurn(
  turns: TurnMark[],
  currentIndex: number,
  direction: 'prev' | 'next',
): number | undefined {
  if (turns.length === 0) return undefined

  if (direction === 'next') {
    return turns.find((turn) => turn.index > currentIndex)?.index
  }

  const before = turns.filter((turn) => turn.index < currentIndex)
  return before[before.length - 1]?.index
}
