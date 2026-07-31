// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'
import { isEditableTarget, matchesShortcut, SHORTCUTS, shortcutLabel } from './shortcuts.js'

describe('shortcuts', () => {
  it('matches the primary modifier on macOS and Windows without stealing shifted variants', () => {
    expect(
      matchesShortcut(
        new KeyboardEvent('keydown', { key: 'k', metaKey: true }),
        SHORTCUTS.commandPalette,
      ),
    ).toBe(true)
    expect(
      matchesShortcut(
        new KeyboardEvent('keydown', { key: 'k', ctrlKey: true }),
        SHORTCUTS.commandPalette,
      ),
    ).toBe(true)
    expect(
      matchesShortcut(
        new KeyboardEvent('keydown', { key: 'k', metaKey: true, shiftKey: true }),
        SHORTCUTS.commandPalette,
      ),
    ).toBe(false)
  })

  it('formats platform-native hints and recognizes every editable target', () => {
    expect(shortcutLabel(SHORTCUTS.newProject, true)).toBe('⌘⇧O')
    expect(shortcutLabel(SHORTCUTS.newProject, false)).toBe('Ctrl Shift O')
    expect(isEditableTarget(document.createElement('textarea'))).toBe(true)
    expect(isEditableTarget(document.createElement('input'))).toBe(true)

    const editable = document.createElement('div')
    editable.contentEditable = 'true'
    expect(isEditableTarget(editable)).toBe(true)
    expect(isEditableTarget(document.createElement('button'))).toBe(false)
  })
})
