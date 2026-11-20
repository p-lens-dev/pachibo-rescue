import type { BackupView, Problem } from "./backups"

/** An unknown value, in the shape of what is missing. */
const UNKNOWN = "--"

const twoDigits = (value: number): string => String(value).padStart(2, "0")

/** A moment in UNIX seconds as the local date and time, `2026/11/20 12:34`. */
export const madeAtText = (seconds: number | undefined): string => {
  if (seconds === undefined) return `${UNKNOWN}/${UNKNOWN}/${UNKNOWN} ${UNKNOWN}:${UNKNOWN}`
  const date = new Date(seconds * 1000)
  return `${String(date.getFullYear())}/${twoDigits(date.getMonth() + 1)}/${twoDigits(date.getDate())} ${twoDigits(date.getHours())}:${twoDigits(date.getMinutes())}`
}

export const deviceText = (backup: BackupView): string => backup.deviceName ?? UNKNOWN

export const iosText = (backup: BackupView): string => `iOS ${backup.iosVersion ?? UNKNOWN}`

export const problemText = (problem: Problem): string => {
  switch (problem.type) {
    case "permission-denied":
      return `フォルダを読む許可がありません: ${problem.path}`
    case "missing":
      return `ファイルが見つかりません: ${problem.path}`
    case "unreadable":
      return `ファイルを読めませんでした: ${problem.path}（${problem.detail}）`
    case "password-needed":
      return "暗号化されたバックアップです。パスワードを入れてください。"
    case "wrong-password":
      return "パスワードが違います。"
    case "not-in-backup":
      return "このバックアップにはぱち簿のデータが入っていません。"
    case "not-pachibo-data":
      return "ぱち簿のデータを読めませんでした。"
    case "broken":
      return `バックアップを読めませんでした（${problem.detail}）`
    case "write-failed":
      return `書き出せませんでした: ${problem.path}（${problem.detail}）`
  }
}

/** Whether the problem is the Mac refusing the backup folder until the app has Full Disk Access. */
export const needsFullDiskAccess = (problem: Problem): boolean => problem.type === "permission-denied"
