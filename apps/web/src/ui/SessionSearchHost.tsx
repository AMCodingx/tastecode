import {
  forwardRef,
  memo,
  useCallback,
  useImperativeHandle,
  useState,
  type ComponentProps,
} from 'react'
import { SessionSearch } from './SessionSearch.js'

export type SessionSearchHandle = {
  open: (initialProjectPath?: string) => void
  close: () => void
}

type SessionSearchHostProps = Omit<
  ComponentProps<typeof SessionSearch>,
  'initialProjectPath' | 'onClose'
>

const SessionSearchHostComponent = forwardRef<SessionSearchHandle, SessionSearchHostProps>(
  function SessionSearchHost(props, ref) {
    const [request, setRequest] = useState<{ initialProjectPath: string | undefined }>()
    const close = useCallback(() => setRequest(undefined), [])
    const open = useCallback(
      (initialProjectPath?: string) => setRequest({ initialProjectPath }),
      [],
    )

    useImperativeHandle(ref, () => ({ open, close }), [close, open])

    if (!request) return null
    return (
      <SessionSearch
        {...props}
        initialProjectPath={request.initialProjectPath}
        onSelect={(threadId, turnId) => {
          close()
          props.onSelect(threadId, turnId)
        }}
        onClose={close}
      />
    )
  },
)

export const SessionSearchHost = memo(SessionSearchHostComponent)
