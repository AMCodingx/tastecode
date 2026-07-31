import { useEffect, useMemo, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { ShortcutHint } from './ShortcutHint.js'

export type CommandScope = 'all' | 'projects'

export type PaletteCommand = {
  id: string
  title: string
  detail?: string
  group: 'Actions' | 'Projects' | 'Chats'
  keywords?: string
  shortcut?: string
  projectCommand?: boolean
  run: () => void
}

export function CommandPalette(props: {
  commands: PaletteCommand[]
  scope: CommandScope
  onClose: () => void
}) {
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState(0)
  const input = useRef<HTMLInputElement>(null)
  const results = useRef<HTMLDivElement>(null)

  useEffect(() => {
    input.current?.focus()
  }, [])

  const commands = useMemo(() => {
    const available =
      props.scope === 'projects'
        ? props.commands.filter((command) => command.projectCommand)
        : props.commands
    const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean)
    if (terms.length === 0) return available
    return available.filter((command) => {
      const searchable =
        `${command.title} ${command.detail ?? ''} ${command.group} ${command.keywords ?? ''}`.toLowerCase()
      return terms.every((term) => searchable.includes(term))
    })
  }, [props.commands, props.scope, query])

  useEffect(() => {
    setSelected(0)
  }, [query, props.scope])

  useEffect(() => {
    const command = commands[selected]
    if (!command) return
    results.current
      ?.querySelector<HTMLElement>(`#command-${CSS.escape(command.id)}`)
      ?.scrollIntoView?.({ block: 'nearest' })
  }, [commands, selected])

  const choose = (command: PaletteCommand | undefined) => {
    if (!command) return
    props.onClose()
    command.run()
  }

  return (
    <div
      className="command-palette"
      role="dialog"
      aria-modal="true"
      aria-label={props.scope === 'projects' ? 'Switch project' : 'Command palette'}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && !event.defaultPrevented) {
          event.preventDefault()
          props.onClose()
        }
      }}
    >
      <button
        className="command-palette__scrim"
        onClick={props.onClose}
        aria-label="Close command palette"
      />
      <div className="command-palette__panel">
        <div className="command-palette__search">
          <Search size={15} aria-hidden />
          <input
            ref={input}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                event.preventDefault()
                props.onClose()
                return
              }
              if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                event.preventDefault()
                if (commands.length === 0) return
                const direction = event.key === 'ArrowDown' ? 1 : -1
                setSelected((current) => (current + direction + commands.length) % commands.length)
                return
              }
              if (event.key === 'Enter') {
                event.preventDefault()
                choose(commands[selected])
              }
            }}
            placeholder={
              props.scope === 'projects' ? 'Switch project…' : 'Search commands, projects, chats…'
            }
            spellCheck={false}
            aria-label="Search commands"
            aria-controls="command-palette-results"
            aria-activedescendant={
              commands[selected] ? `command-${commands[selected].id}` : undefined
            }
          />
        </div>

        <div
          ref={results}
          className="command-palette__results"
          id="command-palette-results"
          role="listbox"
        >
          {commands.length === 0 ? (
            <p className="command-palette__empty">No matching commands.</p>
          ) : (
            commands.map((command, index) => {
              const startsGroup = index === 0 || commands[index - 1]?.group !== command.group
              return (
                <div key={command.id}>
                  {startsGroup ? <p className="command-palette__group">{command.group}</p> : null}
                  <button
                    id={`command-${command.id}`}
                    className={`command-palette__item ${index === selected ? 'is-selected' : ''}`}
                    onClick={() => choose(command)}
                    onMouseEnter={() => setSelected(index)}
                    role="option"
                    aria-selected={index === selected}
                  >
                    <span className="command-palette__copy">
                      <span className="command-palette__name">{command.title}</span>
                      {command.detail ? (
                        <span className="command-palette__detail">{command.detail}</span>
                      ) : null}
                    </span>
                    {command.shortcut ? <ShortcutHint>{command.shortcut}</ShortcutHint> : null}
                  </button>
                </div>
              )
            })
          )}
        </div>
      </div>
    </div>
  )
}
