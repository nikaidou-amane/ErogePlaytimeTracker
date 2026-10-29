import { describe, expect, it } from 'vitest';
import { backupFilename } from './backupFilename';

describe('backup filename', () => {
  it('includes second precision so exports never collide within a day', () => {
    expect(backupFilename(new Date(2026, 0, 2, 3, 4, 5))).toBe(
      'eroge-playtime-tracker-2026-01-02-03-04-05.eptbackup',
    );
  });

  it('zero pads every date and time part', () => {
    expect(backupFilename(new Date(2026, 10, 9, 8, 7, 6))).toBe(
      'eroge-playtime-tracker-2026-11-09-08-07-06.eptbackup',
    );
  });

  it('separates repeated exports of the same day by their time', () => {
    const first = backupFilename(new Date(2026, 0, 2, 3, 4, 5));
    const second = backupFilename(new Date(2026, 0, 2, 3, 4, 6));
    expect(first).not.toBe(second);
  });

  it('stays a valid Windows file name', () => {
    expect(backupFilename(new Date(2026, 0, 2, 3, 4, 5))).not.toMatch(/[\\/:*?"<>|]/);
  });
});
