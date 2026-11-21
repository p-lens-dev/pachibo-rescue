import { listen, type UnlistenFn } from "@tauri-apps/api/event"

import { called, type Problem, type Result } from "./backups"

/** A connected phone as the app's command describes it. */
export type PhoneView = {
  readonly serial: string
  readonly model: string
  readonly hasPachibo: boolean
  readonly awake: boolean
  /** In airplane mode, so ぱち簿 loads no ads while it is driven. */
  readonly airplane: boolean
}

/** What adb sees, for the screen to guide the person to a ready phone. */
export type Phones =
  | { readonly type: "no-adb" }
  | { readonly type: "none" }
  | { readonly type: "unauthorized" }
  | { readonly type: "ready"; readonly phone: PhoneView }

/** Where a walk has reached, as it reads month by month. */
export type Progress = {
  readonly year: number
  readonly month: number
  readonly records: number
}

/** What a quick look found, to size the job before reading it; the count is null where the phone did not give it plainly. */
export type Survey = {
  readonly records: number | null
}

/** How a walk ended: read to the end, stopped by the person, or unable to go on (what it read before is written all the same). */
export type End = { readonly type: "finished" } | { readonly type: "cancelled" } | { readonly type: "failed"; readonly problem: Problem }

/** What a walk wrote. */
export type Rescued = {
  readonly count: number
  readonly end: End
  /** The months (`2026-03`) whose results did not add up to the month's total, so may be missing a record. */
  readonly unsureMonths: readonly string[]
  /** The run's log, where one was kept: to send along when asking for help. */
  readonly log: string | null
}

/** What adb sees now: a phone connected, allowed and holding ぱち簿, or what is missing. */
export const androidPhones = (): Promise<Result<Phones>> => called<Phones>("android_phones")

/** Count the records over all months without opening them, to size the job. */
export const androidSurvey = (serial: string): Promise<Result<Survey>> => called<Survey>("android_survey", { serial })

/** Read every record off the phone, from this month (`year`/`month`, by this computer's clock) back, and write them as a CSV at `out`. */
export const androidRescue = (serial: string, out: string, deep: boolean, expected: number | null, year: number, month: number): Promise<Result<Rescued>> =>
  called<Rescued>("android_rescue", { serial, out, deep, expected, year, month })

/** Stop a running walk; it keeps what it has gathered so far. */
export const androidCancel = (): Promise<Result<null>> => called<null>("android_cancel")

/** What a read waits on the person for: the phone plugged back, or unlocked, within so many seconds; or back. */
export type Waiting = { readonly type: "unplugged"; readonly seconds: number } | { readonly type: "locked"; readonly seconds: number } | { readonly type: "back" }

/** Hear when a read waits on the person, until the returned function is called. */
export const onWaiting = (hear: (waiting: Waiting) => void): Promise<UnlistenFn> => listen<Waiting>("android-waiting", (event) => hear(event.payload))

/** Hear where the walk has reached, until the returned function is called. */
export const onProgress = (hear: (progress: Progress) => void): Promise<UnlistenFn> => listen<Progress>("android-progress", (event) => hear(event.payload))
