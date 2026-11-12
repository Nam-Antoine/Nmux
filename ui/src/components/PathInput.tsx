import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { call } from '../api'
import { join } from '../paths'
import { FolderIcon } from './icons'

interface Box {
  left: number
  top: number
  width: number
  maxHeight: number
}

export function PathInput(props: {
  machine: number
  value: string
  onChange: (value: string) => void
  onGo: (path: string) => void
  placeholder?: string
}): React.JSX.Element {
  const { machine, value, onChange, onGo } = props
  const id = useId()
  const input = useRef<HTMLInputElement>(null)
  const [live, setLive] = useState<string | null>(null)
  const [active, setActive] = useState(-1)
  const [listings, setListings] = useState<Record<string, string[]>>({})
  const [box, setBox] = useState<Box | null>(null)

  const cut = value.lastIndexOf('/')
  const parent = cut < 0 ? '~' : value.slice(0, cut) || '/'
  const segment = value.slice(cut + 1)
  const key = `${machine}\n${parent}`
  const known = key in listings
  const wanted = live === value && value !== ''

  useEffect(() => {
    if (!wanted || known) return
    const timer = setTimeout(() => {
      call({ op: 'listDir', path: parent, machine })
        .then((reply) => {
          const names = reply.kind === 'dir' ? reply.entries.filter((e) => e.dir).map((e) => e.name) : []
          setListings((all) => ({ ...all, [key]: names }))
        })
        .catch(() => {})
    }, 150)
    return () => clearTimeout(timer)
  }, [wanted, known, key, parent, machine])

  const prefix = segment.toLowerCase()
  const items = wanted
    ? (listings[key] ?? []).filter((n) => n.toLowerCase().startsWith(prefix) && (segment.startsWith('.') || !n.startsWith('.')))
    : []
  const visible = items.length > 0
  const at = active < items.length ? active : -1

  useLayoutEffect(() => {
    if (!visible) return
    const measure = (): void => {
      const r = input.current?.getBoundingClientRect()
      if (r) setBox({ left: r.left, top: r.bottom + 4, width: r.width, maxHeight: Math.min(240, window.innerHeight - r.bottom - 12) })
    }
    measure()
    window.addEventListener('resize', measure)
    window.addEventListener('scroll', measure, true)
    return () => {
      window.removeEventListener('resize', measure)
      window.removeEventListener('scroll', measure, true)
    }
  }, [visible])

  useEffect(() => {
    if (at >= 0) document.getElementById(`${id}-${at}`)?.scrollIntoView({ block: 'nearest' })
  }, [at, id])

  const complete = (name: string): string => {
    const next = `${join(parent, name)}/`
    onChange(next)
    setLive(next)
    setActive(-1)
    return next
  }

  const onKeyDown = (e: React.KeyboardEvent): void => {
    if (e.key === 'Enter' && at < 0) {
      setLive(null)
      onGo(value)
      return
    }
    if (e.key === 'ArrowDown' && !visible) setLive(value)
    const pick = items[Math.max(at, 0)]
    if (pick === undefined) return
    if (e.key === 'Escape') {
      e.stopPropagation()
      setLive(null)
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const step = e.key === 'ArrowDown' ? 1 : -1
      setActive(at < 0 && step < 0 ? items.length - 1 : (at + step + items.length) % items.length)
    } else if (e.key === 'Enter' || (e.key === 'Tab' && !e.shiftKey)) {
      e.preventDefault()
      complete(pick)
    }
  }

  return (
    <div className="path-input">
      <input
        ref={input}
        role="combobox"
        aria-expanded={visible}
        aria-controls={`${id}-list`}
        aria-autocomplete="list"
        aria-activedescendant={at >= 0 ? `${id}-${at}` : undefined}
        value={value}
        onChange={(e) => {
          onChange(e.target.value)
          setLive(e.target.value)
          setActive(-1)
        }}
        onKeyDown={onKeyDown}
        onFocus={() => setListings({})}
        onBlur={() => setLive(null)}
        placeholder={props.placeholder}
        spellCheck={false}
        autoComplete="off"
      />
      {visible &&
        box &&
        createPortal(
          <div className="path-list" role="listbox" id={`${id}-list`} style={box}>
            {items.map((name, i) => (
              <div
                key={name}
                id={`${id}-${i}`}
                role="option"
                aria-selected={i === at}
                className={`path-option${i === at ? ' active' : ''}`}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  onGo(complete(name))
                  setLive(null)
                }}
              >
                <FolderIcon />
                <span>{name}</span>
              </div>
            ))}
          </div>,
          document.body
        )}
    </div>
  )
}
