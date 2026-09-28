// Paths come from the Worker's host OS, so accept both separators: Windows
// paths (including `\\?\` and UNC forms) must keep backslashes, because
// Windows does not translate `/` after a `\\?\` prefix.

function lastSeparatorIndex(path: string): number {
  return Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
}

/** Split typed input into the directory to browse and a lowercase name prefix. */
export function splitPathInput(input: string): { dir: string; namePrefix: string } {
  const index = lastSeparatorIndex(input)
  if (index < 0) return { dir: input, namePrefix: '' }
  return {
    dir: input.slice(0, index + 1),
    namePrefix: input.slice(index + 1).toLowerCase(),
  }
}

/** Append the separator the path already uses (`/` when it has none). */
export function withTrailingSeparator(path: string): string {
  if (path.endsWith('/') || path.endsWith('\\')) return path
  const index = lastSeparatorIndex(path)
  const separator = index >= 0 ? path[index] : '/'
  return `${path}${separator}`
}
