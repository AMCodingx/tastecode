import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import type {
  PullRequestDetail,
  PullRequestListItem,
  PullRequestListResult,
} from '@harness/contracts'
import {
  CircleAlert,
  GitMerge,
  GitPullRequest,
  GitPullRequestClosed,
  GitPullRequestDraft,
  Inbox,
  ListFilter,
  RefreshCw,
  Search,
} from 'lucide-react'
import type { Transport } from '../../transport.js'
import { Menu, MenuItem } from '../Menu.js'
import { PullRequestDetailPane } from './PullRequestDetailPane.js'
import './pull-requests.css'

type PullRequestFilter = 'all' | 'reviewing' | 'authored'
type PullRequestStatusFilter = 'all' | 'open' | 'draft' | 'merged' | 'closed'
const LIST_REVALIDATE_AFTER_MS = 30_000

export function PullRequestsView(props: {
  transport: Transport
  onOpenChat: (pullRequest: PullRequestListItem) => void
}) {
  const [result, setResult] = useState<PullRequestListResult>()
  const [error, setError] = useState<string>()
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [filter, setFilter] = useState<PullRequestFilter>('all')
  const [statusFilter, setStatusFilter] = useState<PullRequestStatusFilter>('all')
  const [query, setQuery] = useState('')
  const [selectedKey, setSelectedKey] = useState<string>()
  const request = useRef(0)

  const load = useCallback(
    async (refresh = false): Promise<PullRequestListResult | undefined> => {
      const id = ++request.current
      refresh ? setRefreshing(true) : setLoading(true)
      setError(undefined)
      try {
        const next = await props.transport.request('pullRequests.list', { refresh })
        if (id !== request.current) return
        setResult(next)
        setSelectedKey((current) =>
          current && next.items.some((item) => pullRequestKey(item) === current)
            ? current
            : next.items[0]
              ? pullRequestKey(next.items[0])
              : undefined,
        )
        return next
      } catch (cause) {
        if (id === request.current) setError(messageOf(cause))
      } finally {
        if (id === request.current) {
          setLoading(false)
          setRefreshing(false)
        }
      }
      return undefined
    },
    [props.transport],
  )

  useEffect(() => {
    void load().then((next) => {
      if (next && Date.now() - next.fetchedAt > LIST_REVALIDATE_AFTER_MS) void load(true)
    })
    return () => {
      request.current += 1
    }
  }, [load])

  const normalizedQuery = query.trim().toLowerCase()
  const relationshipItems = useMemo(
    () =>
      (result?.items ?? []).filter(
        (item) => filter === 'all' || item.relationship === filter || item.relationship === 'both',
      ),
    [filter, result?.items],
  )
  const visible = useMemo(
    () =>
      relationshipItems.filter((item) => {
        if (!statusMatches(item, statusFilter)) return false
        if (!normalizedQuery) return true
        return [
          item.title,
          item.repository,
          item.headRefName,
          String(item.number),
          listStateLabel(item),
        ].some((value) => value.toLowerCase().includes(normalizedQuery))
      }),
    [normalizedQuery, relationshipItems, statusFilter],
  )

  useEffect(() => {
    if (visible.length === 0) return
    if (!visible.some((item) => pullRequestKey(item) === selectedKey)) {
      setSelectedKey(pullRequestKey(visible[0]!))
    }
  }, [selectedKey, visible])

  const syncDetail = useCallback((next: PullRequestDetail) => {
    setResult((current) => {
      if (!current) return current
      const key = pullRequestKey(next)
      return {
        ...current,
        items: current.items
          .map((item) => (pullRequestKey(item) === key ? listItemFromDetail(next) : item))
          .sort((left, right) => Date.parse(right.updatedAt) - Date.parse(left.updatedAt)),
      }
    })
  }, [])

  const selected = visible.find((item) => pullRequestKey(item) === selectedKey)
  const counts = useMemo(() => {
    const statusItems = (result?.items ?? []).filter((item) => statusMatches(item, statusFilter))
    return {
      all: statusItems.length,
      reviewing: statusItems.filter(
        (item) => item.relationship === 'reviewing' || item.relationship === 'both',
      ).length,
      authored: statusItems.filter(
        (item) => item.relationship === 'authored' || item.relationship === 'both',
      ).length,
    }
  }, [result?.items, statusFilter])
  const statusCounts = useMemo(
    () => ({
      all: relationshipItems.length,
      open: relationshipItems.filter((item) => statusMatches(item, 'open')).length,
      draft: relationshipItems.filter((item) => statusMatches(item, 'draft')).length,
      merged: relationshipItems.filter((item) => statusMatches(item, 'merged')).length,
      closed: relationshipItems.filter((item) => statusMatches(item, 'closed')).length,
    }),
    [relationshipItems],
  )

  return (
    <section className="pr-workspace" aria-label="Pull requests">
      <aside className="pr-list-pane">
        <header className="pr-list-head">
          <div className="pr-list-title-row">
            <div>
              <h1>Pull requests</h1>
              <span>{result?.account.login ? `@${result.account.login}` : 'GitHub'}</span>
            </div>
            <button
              type="button"
              className="pr-icon-button"
              aria-label="Refresh pull requests"
              title="Refresh pull requests"
              disabled={refreshing}
              onClick={() => void load(true)}
            >
              <RefreshCw size={14} className={refreshing ? 'is-spinning' : undefined} aria-hidden />
            </button>
          </div>

          <div className="pr-segment" role="tablist" aria-label="Pull request relationship">
            {(['all', 'reviewing', 'authored'] as const).map((value) => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-selected={filter === value}
                className={filter === value ? 'is-active' : undefined}
                onClick={() => setFilter(value)}
              >
                {capitalize(value)}
                <span>{counts[value]}</span>
              </button>
            ))}
          </div>

          <div className="pr-search-row">
            <label className="pr-search">
              <Search size={14} aria-hidden />
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Search pull requests"
                aria-label="Search pull requests"
              />
            </label>
            <StatusFilterMenu
              value={statusFilter}
              counts={statusCounts}
              onChange={setStatusFilter}
            />
          </div>
        </header>

        <div className="pr-list-scroll">
          {loading ? (
            <PullRequestListSkeleton />
          ) : error && !result ? (
            <ListMessage
              icon={<CircleAlert size={18} aria-hidden />}
              title="Couldn't load pull requests"
              detail={error}
              action="Try again"
              onAction={() => void load(true)}
            />
          ) : !result?.account.authenticated ? (
            <ListMessage
              icon={<GitPullRequest size={19} aria-hidden />}
              title={result?.account.available ? 'Connect GitHub' : 'Install GitHub CLI'}
              detail={
                result?.account.error ??
                'Harness uses your local GitHub CLI session and never reads its token.'
              }
              action="Open setup guide"
              href="https://cli.github.com/manual/gh_auth_login"
            />
          ) : visible.length === 0 ? (
            <ListMessage
              icon={<Inbox size={19} aria-hidden />}
              title={
                normalizedQuery
                  ? 'No matching pull requests'
                  : statusFilter === 'all'
                    ? 'No pull requests'
                    : `No ${statusFilterLabel(statusFilter).toLowerCase()} pull requests`
              }
              detail={
                normalizedQuery
                  ? 'Try a title, repository, status, or pull-request number.'
                  : filter === 'reviewing'
                    ? statusFilter === 'open'
                      ? 'Nothing is waiting for your review.'
                      : 'No pull requests in this state requested or received your review.'
                    : statusFilter === 'all'
                      ? 'Authored, review-requested, and reviewed pull requests will appear here.'
                      : `No ${statusFilterLabel(statusFilter).toLowerCase()} pull requests are in this view.`
              }
            />
          ) : (
            <>
              <PullRequestGroup
                title={filter === 'all' ? undefined : capitalize(filter)}
                items={visible}
                selectedKey={selectedKey}
                onSelect={setSelectedKey}
              />
              {result.truncated ? (
                <p className="pr-list-note">
                  GitHub capped one or more searches, so some older pull requests may be omitted.
                </p>
              ) : null}
            </>
          )}
        </div>
      </aside>

      <div className="pr-detail-pane">
        {selected ? (
          <PullRequestDetailPane
            key={pullRequestKey(selected)}
            item={selected}
            transport={props.transport}
            onOpenChat={() => props.onOpenChat(selected)}
            onChanged={syncDetail}
          />
        ) : (
          <div className="pr-detail-empty">
            <span className="pr-empty-emblem">
              <GitPullRequest size={22} aria-hidden />
            </span>
            <strong>Select a pull request</strong>
            <p>Summary, files, review conversations, checks, and merge controls live here.</p>
          </div>
        )}
      </div>
    </section>
  )
}

function PullRequestGroup(props: {
  title: string | undefined
  items: PullRequestListItem[]
  selectedKey: string | undefined
  onSelect: (key: string) => void
}) {
  return (
    <section className="pr-list-group">
      {props.title ? <h2>{props.title}</h2> : null}
      <div className="pr-list-items">
        {props.items.map((item) => {
          const key = pullRequestKey(item)
          const stateLabel = listStateLabel(item)
          return (
            <button
              type="button"
              className={`pr-list-item${props.selectedKey === key ? ' is-selected' : ''}`}
              aria-current={props.selectedKey === key ? 'true' : undefined}
              key={key}
              onClick={() => props.onSelect(key)}
            >
              <span
                className={`pr-state-mark is-${listStatus(item)}`}
                aria-label={stateLabel}
                title={stateLabel}
              >
                {item.isDraft ? (
                  <GitPullRequestDraft size={15} aria-hidden />
                ) : item.state === 'MERGED' ? (
                  <GitMerge size={15} aria-hidden />
                ) : item.state === 'CLOSED' ? (
                  <GitPullRequestClosed size={15} aria-hidden />
                ) : (
                  <GitPullRequest size={15} aria-hidden />
                )}
              </span>
              <span className="pr-list-copy">
                <span className="pr-list-item-head">
                  <strong>{item.title}</strong>
                  <time dateTime={item.updatedAt}>{relativeTime(item.updatedAt)}</time>
                </span>
                <span className="pr-list-meta">
                  <span>{item.repository}</span>
                  <span>#{item.number}</span>
                  {item.isDraft || item.state !== 'OPEN' ? (
                    <span className={`pr-list-state is-${listStatus(item)}`}>{stateLabel}</span>
                  ) : null}
                  {item.headRefName ? (
                    <span className="pr-list-branch">{item.headRefName}</span>
                  ) : null}
                </span>
              </span>
              {item.headRefName ? (
                <span
                  className="pr-list-stats"
                  aria-label={`${item.additions} additions and ${item.deletions} deletions`}
                >
                  <span className="is-addition">+{formatCount(item.additions)}</span>
                  <span className="is-deletion">−{formatCount(item.deletions)}</span>
                </span>
              ) : null}
            </button>
          )
        })}
      </div>
    </section>
  )
}

function StatusFilterMenu(props: {
  value: PullRequestStatusFilter
  counts: Record<PullRequestStatusFilter, number>
  onChange: (value: PullRequestStatusFilter) => void
}) {
  const options: Array<{
    value: PullRequestStatusFilter
    icon: ReactNode
  }> = [
    { value: 'all', icon: <ListFilter size={13} aria-hidden /> },
    { value: 'open', icon: <GitPullRequest size={13} aria-hidden /> },
    { value: 'draft', icon: <GitPullRequestDraft size={13} aria-hidden /> },
    { value: 'merged', icon: <GitMerge size={13} aria-hidden /> },
    { value: 'closed', icon: <GitPullRequestClosed size={13} aria-hidden /> },
  ]

  return (
    <Menu
      align="right"
      drop="down"
      label={`Filter by status: ${statusFilterLabel(props.value)}`}
      triggerClassName={`pr-status-filter${props.value === 'all' ? '' : ' is-active'}`}
      trigger={() => (
        <span>
          <ListFilter size={13} aria-hidden />
          {statusFilterLabel(props.value)}
        </span>
      )}
    >
      {(close) =>
        options.map((option) => (
          <MenuItem
            key={option.value}
            title={statusFilterLabel(option.value)}
            detail={`${props.counts[option.value].toLocaleString()} pull requests`}
            icon={option.icon}
            active={props.value === option.value}
            onClick={() => {
              props.onChange(option.value)
              close()
            }}
          />
        ))
      }
    </Menu>
  )
}

function PullRequestListSkeleton() {
  return (
    <div className="pr-list-skeleton" aria-label="Loading pull requests">
      <span className="pr-skeleton-title" />
      {[0, 1, 2, 3].map((index) => (
        <span className="pr-skeleton-row" key={index}>
          <span />
          <span />
        </span>
      ))}
    </div>
  )
}

function ListMessage(props: {
  icon: ReactNode
  title: string
  detail: string
  action?: string
  href?: string
  onAction?: () => void
}) {
  return (
    <div className="pr-list-message">
      <span className="pr-empty-emblem">{props.icon}</span>
      <strong>{props.title}</strong>
      <p>{props.detail}</p>
      {props.action && props.href ? (
        <a className="pr-button is-secondary" href={props.href} target="_blank" rel="noreferrer">
          {props.action}
        </a>
      ) : props.action ? (
        <button type="button" className="pr-button is-secondary" onClick={props.onAction}>
          {props.action}
        </button>
      ) : null}
    </div>
  )
}

function listItemFromDetail(detail: PullRequestDetail): PullRequestListItem {
  return {
    id: detail.id,
    repository: detail.repository,
    number: detail.number,
    title: detail.title,
    url: detail.url,
    author: detail.author,
    updatedAt: detail.updatedAt,
    isDraft: detail.isDraft,
    state: detail.state,
    additions: detail.additions,
    deletions: detail.deletions,
    commentsCount: detail.commentsCount,
    headRefName: detail.headRefName,
    baseRefName: detail.baseRefName,
    ...(detail.reviewDecision ? { reviewDecision: detail.reviewDecision } : {}),
    ...(detail.mergeStateStatus ? { mergeStateStatus: detail.mergeStateStatus } : {}),
    relationship: detail.relationship,
    ...(detail.localProjectPath ? { localProjectPath: detail.localProjectPath } : {}),
  }
}

function pullRequestKey(item: PullRequestListItem): string {
  return `${item.repository.toLowerCase()}#${item.number}`
}

function listStatus(item: PullRequestListItem): string {
  if (item.isDraft && item.state === 'CLOSED') return 'closed-draft'
  if (item.isDraft) return 'draft'
  if (item.state === 'MERGED') return 'merged'
  if (item.state === 'CLOSED') return 'closed'
  if (item.mergeStateStatus === 'DIRTY') return 'conflict'
  if (item.reviewDecision === 'CHANGES_REQUESTED') return 'attention'
  if (item.reviewDecision === 'APPROVED') return 'success'
  return 'open'
}

function listStateLabel(item: PullRequestListItem): string {
  if (item.isDraft && item.state === 'CLOSED') return 'Closed draft'
  if (item.isDraft) return 'Draft'
  if (item.state === 'MERGED') return 'Merged'
  if (item.state === 'CLOSED') return 'Closed'
  return 'Open'
}

function statusMatches(item: PullRequestListItem, filter: PullRequestStatusFilter): boolean {
  if (filter === 'all') return true
  if (filter === 'draft') return item.isDraft
  if (filter === 'open') return item.state === 'OPEN'
  if (filter === 'merged') return item.state === 'MERGED'
  return item.state === 'CLOSED'
}

function statusFilterLabel(filter: PullRequestStatusFilter): string {
  if (filter === 'all') return 'All states'
  if (filter === 'draft') return 'Drafts'
  return capitalize(filter)
}

function formatCount(value: number): string {
  return value >= 10_000
    ? `${Math.round(value / 1_000)}k`
    : value >= 1_000
      ? `${(value / 1_000).toFixed(1)}k`
      : String(value)
}

function relativeTime(value: string): string {
  const elapsed = Date.now() - Date.parse(value)
  const minutes = Math.max(0, Math.floor(elapsed / 60_000))
  if (minutes < 1) return 'now'
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d`
  const months = Math.floor(days / 30)
  if (months < 12) return `${months}mo`
  return `${Math.floor(months / 12)}y`
}

function capitalize(value: string): string {
  return `${value.slice(0, 1).toUpperCase()}${value.slice(1)}`
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
