import { memo } from 'react'
import { GitBranch, History, SquareTerminal } from 'lucide-react'

/**
 * Header above the thread: the session title and its workspace tools.
 */
function StageHeaderComponent(props: {
  title: string | undefined
  checkpointCount: number
  worktreeBranch: string | undefined
  terminalOpen: boolean
  onOpenRollback: () => void
  onToggleTerminal: () => void
}) {
  return (
    <header className="stagehead">
      {props.title ? <span className="stagehead__title">{props.title}</span> : null}

      <div className="stagehead__tools">
        {props.title ? (
          <button
            className={`ghost terminal-trigger${props.terminalOpen ? ' is-open' : ''}`}
            aria-pressed={props.terminalOpen}
            onClick={props.onToggleTerminal}
          >
            <SquareTerminal size={13} aria-hidden />
            Terminal
          </button>
        ) : null}
        {props.worktreeBranch ? (
          <span className="worktree-branch" title="Isolated checkout">
            <GitBranch size={12} aria-hidden />
            {props.worktreeBranch}
          </span>
        ) : null}
        {props.checkpointCount > 0 ? (
          <button className="ghost rollback-trigger" onClick={props.onOpenRollback}>
            <History size={12} aria-hidden />
            {props.checkpointCount} checkpoint{props.checkpointCount === 1 ? '' : 's'}
          </button>
        ) : null}
      </div>
    </header>
  )
}

/**
 * Memoised: the app root re-renders on every streamed frame, and this subtree
 * does not change while an answer arrives. Stable owner callbacks let the
 * shallow comparison keep header controls out of that path.
 */
export const StageHeader = memo(StageHeaderComponent)
