export function join(dir: string, name: string): string {
  const sep = dir.includes('\\') ? '\\' : '/'
  return dir.endsWith(sep) ? dir + name : dir + sep + name
}

export function tail(path: string): string {
  return path.split(/[\\/]/).pop() || path
}

export function up(path: string): string {
  return path.replace(/\/[^/]+\/*$/, '') || '/'
}

export function tidy(path: string): string {
  return path.trim().replace(/(.)\/+$/, '$1')
}
