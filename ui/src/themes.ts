import type { ITheme } from '@xterm/xterm'
import type { Settings } from './types'

const dark: ITheme = {
  background: '#17191d',
  foreground: '#d7dae0',
  cursor: '#d7dae0',
  cursorAccent: '#17191d',
  selectionBackground: '#2f3644',
  black: '#1b1f2a',
  red: '#f07178',
  green: '#98c379',
  yellow: '#e5c07b',
  blue: '#7aa2f7',
  magenta: '#c678dd',
  cyan: '#56b6c2',
  white: '#c8ccd4',
  brightBlack: '#5c6370',
  brightRed: '#ff8a8f',
  brightGreen: '#b5e890',
  brightYellow: '#f5d68a',
  brightBlue: '#9ab8ff',
  brightMagenta: '#d99cf0',
  brightCyan: '#7fd8e0',
  brightWhite: '#ffffff'
}

const light: ITheme = {
  background: '#ffffff',
  foreground: '#24292f',
  cursor: '#24292f',
  cursorAccent: '#ffffff',
  selectionBackground: '#cfe0f7',
  black: '#24292f',
  red: '#cf222e',
  green: '#1a7f37',
  yellow: '#9a6700',
  blue: '#0969da',
  magenta: '#8250df',
  cyan: '#1b7c83',
  white: '#6e7781',
  brightBlack: '#57606a',
  brightRed: '#a40e26',
  brightGreen: '#2da44e',
  brightYellow: '#bf8700',
  brightBlue: '#218bff',
  brightMagenta: '#a475f9',
  brightCyan: '#3192aa',
  brightWhite: '#8c959f'
}

export const THEMES: Array<{ id: string; label: string; terminal: ITheme }> = [
  { id: 'dark', label: 'Dark', terminal: dark },
  { id: 'black', label: 'Black', terminal: { ...dark, background: '#050506', cursorAccent: '#050506' } },
  { id: 'light', label: 'Light', terminal: light }
]

export const PANE_FONTS = ['Cascadia Mono', 'Cascadia Code', 'Consolas', 'JetBrains Mono', 'Fira Code', 'Source Code Pro', 'Hack', 'IBM Plex Mono', 'Lucida Console', 'Courier New']
export const APP_FONTS = ['Segoe UI', 'Inter', 'Roboto', 'Arial', 'Verdana', 'Tahoma']

const PANE_STACK = '"Cascadia Mono", Consolas, "Courier New", monospace'

export function paneLook(settings: Settings): { theme: ITheme; fontFamily: string; fontSize: number } {
  const theme = THEMES.find((t) => t.id === settings.theme)?.terminal ?? dark
  return { theme, fontFamily: `${JSON.stringify(settings.paneFont || 'Geist Mono Variable')}, ${PANE_STACK}`, fontSize: settings.textSize }
}

export function dress(settings: Settings): void {
  const root = document.documentElement
  root.dataset.theme = settings.theme
  for (const [name, font] of [['--pane-font', settings.paneFont], ['--app-font', settings.appFont]] as const) {
    if (font) root.style.setProperty(name, JSON.stringify(font))
    else root.style.removeProperty(name)
  }
}

export function installed(fonts: string[]): string[] {
  const pen = document.createElement('canvas').getContext('2d')
  if (!pen) return fonts
  const width = (family: string): number => {
    pen.font = `72px ${family}`
    return pen.measureText('mmmwwwiiilll0O1').width
  }
  return fonts.filter((font) => ['monospace', 'serif'].some((plain) => width(`${JSON.stringify(font)}, ${plain}`) !== width(plain)))
}
