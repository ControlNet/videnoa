import { describe, expect, it } from 'vitest';
import { formatErrorWithPrefix, getErrorMessage } from '../presentation-error';

describe('presentation-error helpers', () => {
  it('extracts message from Error instances', () => {
    expect(getErrorMessage(new Error('boom'))).toBe('boom');
  });

  it('extracts message from plain objects', () => {
    expect(getErrorMessage({ message: 'plain-object-error' })).toBe('plain-object-error');
  });

  it('falls back to Unknown error for untyped values', () => {
    expect(getErrorMessage(null)).toBe('Unknown error');
  });

  it('formats wrapper text while preserving raw detail text', () => {
    expect(formatErrorWithPrefix('Failed to submit job', new Error('backend detail'))).toBe(
      'Failed to submit job: backend detail',
    );
  });
});
