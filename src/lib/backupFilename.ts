const prefix = 'eroge-playtime-tracker';

function twoDigits(value: number) {
  return String(value).padStart(2, '0');
}

/**
 * Default file name offered when exporting data. Second precision keeps repeated
 * exports on the same day from colliding in the save dialog or the target folder.
 * Colons are avoided because Windows rejects them in file names.
 */
export function backupFilename(date = new Date()) {
  const day = `${date.getFullYear()}-${twoDigits(date.getMonth() + 1)}-${twoDigits(date.getDate())}`;
  const time = `${twoDigits(date.getHours())}-${twoDigits(date.getMinutes())}-${twoDigits(date.getSeconds())}`;
  return `${prefix}-${day}-${time}.eptbackup`;
}
