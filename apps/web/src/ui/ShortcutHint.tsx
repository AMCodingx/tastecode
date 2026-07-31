export function ShortcutHint(props: { children: string; className?: string }) {
  return (
    <kbd className={`shortcut${props.className ? ` ${props.className}` : ''}`} aria-hidden="true">
      {props.children}
    </kbd>
  )
}
