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
  | { readonly type: "scrape-failed"; readonly detail: string; readonly log: string | null }
  | { readonly type: "phone-lost"; readonly detail: string; readonly log: string | null }
  | { readonly type: "adb-missing"; readonly detail: string }
  | { readonly type: "busy" }
  | { readonly type: "cancelled" }

/** Every kind of problem the app's commands report, to tell one from anything else thrown. */
const PROBLEM_TYPES: ReadonlySet<string> = new Set<Problem["type"]>([
  "permission-denied",
  "missing",
  "unreadable",
  "password-needed",
  "wrong-password",
  "not-in-backup",
  "not-pachibo-data",
  "broken",
  "write-failed",
  "scrape-failed",
  "phone-lost",
  "adb-missing",
  "busy",
  "cancelled",
])

/** Words for anything thrown, whatever it is. */
const describe = (thrown: unknown): string => {
  if (thrown instanceof Error) return thrown.message
  if (typeof thrown === "string") return thrown
  try {
    return JSON.stringify(thrown) ?? String(thrown)
  } catch {
    return String(thrown)
  }
}

/** Anything a command threw, as a problem: one the app reported as it is, anything else (a command missing, a bridge failure, a value not foreseen) as broken with its words, so nothing is shown blank. */
export const problemOf = (thrown: unknown): Problem =>
  typeof thrown === "object" && thrown !== null && "type" in thrown && typeof thrown.type === "string" && PROBLEM_TYPES.has(thrown.type)
    ? (thrown as Problem)
    : { type: "broken", detail: describe(thrown) }

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

/** Run a command, its failure (whatever was thrown) as a problem rather than a throw. */
export const called = async <T,>(command: string, args: Record<string, unknown> = {}): Promise<Result<T>> => {
  try {
    return { ok: true, value: await invoke<T>(command, args) }
  } catch (thrown) {
    return { ok: false, problem: problemOf(thrown) }
  }
}

const views = async (command: string, args: Record<string, unknown>): Promise<Result<readonly BackupView[]>> =>
  ((result) => (result.ok ? { ok: true as const, value: result.value.map(viewOf) } : result))(await called<readonly Received[]>(command, args))

/** Whether this computer has a usual place for backups, as Finder, iTunes and Apple Devices do: none on Linux. */
export const hasUsualPlace = (): Promise<boolean> => invoke<boolean>("has_usual_place")

/** The backups where Finder, iTunes and Apple Devices keep them, newest first. */
export const findBackups = (): Promise<Result<readonly BackupView[]>> => views("find_backups", {})

/** The backups in a folder the person chose, newest first. */
export const backupsInFolder = (path: string): Promise<Result<readonly BackupView[]>> => views("backups_in_folder", { path })

/** ぱち簿's records out of a backup written to `out`; how many tallies they hold. */
export const rescue = (path: string, password: string | undefined, out: string): Promise<Result<number>> =>
  called<number>("rescue", { path, password: password ?? null, out })
