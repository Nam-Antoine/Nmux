export function ContextMenu({ at, onClose, children }: { at: { x: number; y: number }; onClose: () => void; children: React.ReactNode }): React.JSX.Element {
  const dismiss = (e: React.MouseEvent): void => {
    e.preventDefault()
    e.stopPropagation()
    onClose()
  }
  return (
    <div onClick={dismiss} onContextMenu={dismiss} onDoubleClick={(e) => e.stopPropagation()}>
      <div className="menu-backdrop" />
      <div className="menu context" style={{ left: Math.min(at.x, window.innerWidth - 200), top: Math.min(at.y, window.innerHeight - 150) }}>
        {children}
      </div>
    </div>
  )
}
