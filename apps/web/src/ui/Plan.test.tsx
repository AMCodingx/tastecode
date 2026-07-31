// @vitest-environment happy-dom
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { Plan } from './Plan.js'

describe('live plan status', () => {
  it('shows only the current step while the response is working', () => {
    render(
      <Plan
        compact
        steps={[
          { text: 'Read the renderer', status: 'done' },
          { text: 'Match the working state', status: 'running' },
          { text: 'Run checks', status: 'pending' },
        ]}
      />,
    )

    expect(screen.getByText('Match the working state')).toBeTruthy()
    expect(screen.queryByText('Read the renderer')).toBeNull()
    expect(screen.queryByText('Plan')).toBeNull()
  })
})
