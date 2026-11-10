import { useEffect, useRef, useState } from 'react'

/** A text field that replaces a label while it is being renamed. */
export function InlineEdit({ value, onDone }: { value: string; onDone: (value: string | null) => void }): React.JSX.Element {
  const [text, setText] = useState(value)
  const ref = useRef<HTMLInputElement>(null)
  useEffect(() => {
    ref.current?.focus()
    ref.current?.select()
  }, [])
  return (
    <input
      ref={ref}
      className="inline-edit"
      value={text}
      maxLength={80}
      onChange={(e) => setText(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onBlur={() => onDone(text.trim() || null)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') onDone(text.trim() || null)
        else if (e.key === 'Escape') onDone(null)
      }}
    />
  )
}
