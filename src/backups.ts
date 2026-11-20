import { invoke } from "@tauri-apps/api/core"

/** A backup as the app's commands describe it. */
export type BackupView = {
  readonly path: string
  readonly deviceName: string | undefined
  readonly productType: string | undefined
  readonly iosVersion: string | undefined
  /** When it was made, in UNIX seconds. */
  readonly madeAt: number | undefined
  readonly encrypted: boolean
  readonly hasPachibo: boolean
}

/** What went wrong, as the app's commands report it. */
export type Problem =
  | { readonly type: "permission-denied"; readonly path: string }
  | { readonly type: "missing"; readonly path: string }
  | { readonly type: "unreadable"; readonly path: string; readonly detail: string }
  | { readonly type: "password-needed" }
  | { readonly type: "wrong-password" }
  | { readonly type: "not-in-backup" }
  | { readonly type: "not-pachibo-data" }
  | { readonly type: "broken"; readonly detail: string }
  | { readonly type: "write-failed"; readonly path: string; readonly detail: string }

export type Result<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly problem: Problem }

type Received = Omit<BackupView, "deviceName" | "productType" | "iosVersion" | "madeAt"> & {
  readonly deviceName: string | null
  readonly productType: string | null
  readonly iosVersion: string | null
  readonly madeAt: number | null
}

const viewOf = (received: Received): BackupView => ({
  ...received,
  deviceName: received.deviceName ?? undefined,
  productType: received.productType ?? undefined,
  iosVersion: received.iosVersion ?? undefined,
  madeAt: received.madeAt ?? undefined,
})

const called = async <T,>(command: string, args: Record<string, unknown>): Promise<Result<T>> => {
  try {
    return { ok: true, value: await invoke<T>(command, args) }
  } catch (problem) {
    return { ok: false, problem: problem as Problem }
  }
}

const views = async (command: string, args: Record<string, unknown>): Promise<Result<readonly BackupView[]>> =>
  ((result) => (result.ok ? { ok: true as const, value: result.value.map(viewOf) } : result))(await called<readonly Received[]>(command, args))

/** The backups where Finder, iTunes and Apple Devices keep them, newest first. */
export const findBackups = (): Promise<Result<readonly BackupView[]>> => views("find_backups", {})

/** The backups in a folder the person chose, newest first. */
export const backupsInFolder = (path: string): Promise<Result<readonly BackupView[]>> => views("backups_in_folder", { path })

/** ぱち簿's records out of a backup written to `out`; how many tallies they hold. */
export const rescue = (path: string, password: string | undefined, out: string): Promise<Result<number>> =>
  called<number>("rescue", { path, password: password ?? null, out })
