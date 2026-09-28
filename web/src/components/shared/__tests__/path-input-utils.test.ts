import { describe, expect, it } from 'vitest'
import { splitPathInput, withTrailingSeparator } from '../path-input-utils'

describe('splitPathInput', () => {
  it('splits POSIX input into directory and lowercase name prefix', () => {
    expect(splitPathInput('/home/user/Do')).toEqual({ dir: '/home/user/', namePrefix: 'do' })
  })

  it('splits Windows input on backslashes', () => {
    expect(splitPathInput('G:\\videnoa\\Te')).toEqual({ dir: 'G:\\videnoa\\', namePrefix: 'te' })
  })

  it('uses whichever separator comes last in mixed input', () => {
    expect(splitPathInput('G:/videnoa\\test data/cl')).toEqual({
      dir: 'G:/videnoa\\test data/',
      namePrefix: 'cl',
    })
  })

  it('keeps input without a separator as the directory', () => {
    expect(splitPathInput('~')).toEqual({ dir: '~', namePrefix: '' })
  })
})

describe('withTrailingSeparator', () => {
  it('appends a slash to POSIX directories', () => {
    expect(withTrailingSeparator('/home/Documents')).toBe('/home/Documents/')
  })

  it('appends a backslash to Windows directories', () => {
    expect(withTrailingSeparator('G:\\videnoa\\test data')).toBe('G:\\videnoa\\test data\\')
  })

  it('keeps UNC and verbatim paths on backslashes', () => {
    expect(withTrailingSeparator('\\\\server\\share\\clips')).toBe('\\\\server\\share\\clips\\')
    expect(withTrailingSeparator('\\\\?\\G:\\videnoa')).toBe('\\\\?\\G:\\videnoa\\')
  })

  it('leaves an existing trailing separator alone', () => {
    expect(withTrailingSeparator('/home/')).toBe('/home/')
    expect(withTrailingSeparator('G:\\')).toBe('G:\\')
  })
})
