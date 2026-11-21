import { save } from "@tauri-apps/plugin-dialog"
import { listen } from "@tauri-apps/api/event"
import { createSignal, Match, onCleanup, onMount, Show, Switch, type JSX } from "solid-js"

import { androidCancel, androidPhones, androidRescue, androidSurvey, onProgress, onWaiting, type Phones, type Progress, type Rescued, type Waiting } from "./android"
import { problemOf, type Problem } from "./backups"
import { problemText } from "./wording"

type Rescue =
  | { readonly type: "idle" }
  | { readonly type: "surveying" }
  | { readonly type: "rescuing"; readonly total: number | null; readonly perRecord: number; readonly progress: Progress | undefined }
  | { readonly type: "done"; readonly rescued: Rescued; readonly expected: number | null; readonly out: string }
  | { readonly type: "failed"; readonly problem: Problem }

const chosenPath = (chosen: string | string[] | null): string | undefined => (typeof chosen === "string" ? chosen : undefined)

/** Seconds each record roughly takes, months turned included: opening a record, reading it and checking the way back (deep) is slower than reading only the day's list. Measured on a real phone: 113 records over ten years read deep in 17 min 40 s. */
const perRecordSeconds = (deep: boolean): number => (deep ? 8 : 3)

/** A rough length of time in words: under a minute, or about so many minutes. */
const roughTime = (seconds: number): string => (seconds < 60 ? "1 分ほど" : `約 ${String(Math.ceil(seconds / 60))} 分`)

const count = (value: number): string => value.toLocaleString("ja-JP")

/** `2026-03` as `2026年3月`. */
const monthText = (month: string): string => {
  const [year, number] = month.split("-")
  return `${year ?? ""}年${String(Number(number))}月`
}

/** Whether this computer runs Windows, which (unlike a Mac) needs the phone maker's USB driver before adb can see the phone. */
const onWindows = navigator.userAgent.includes("Windows")

/** Where a Windows computer cannot see the phone: the phone maker's USB driver is likely missing. */
const WindowsDriver = (): JSX.Element => (
  <Show when={onWindows}>
    <p class="quiet">
      Windows では、スマホのメーカーの「USB ドライバ」（ADB ドライバ）が要ることがあります。つないでも見つからないとき、またはスマホに「USB デバッグを許可しますか？」が出ないときは、メーカーのサポートページ（「機種名 USB ドライバ」で検索）からドライバを入れ、スマホをつなぎ直してください。Pixel の場合は Google の「Google USB ドライバ」です。
    </p>
  </Show>
)

const Steps = (): JSX.Element => (
  <section class="intro">
    <h1>Android のぱち簿のデータを取り出す</h1>
    <p>スマホの画面を読み取って、ぱち簿の収支を取り出し、P-Lens の「設定 → インポート → 汎用形式」で読み込める CSV にします。スマホやどこにもデータを送りません。課金やアプリの書き出しは要りません。</p>
    <ol>
      <li>スマホで「USB デバッグ」をオンにします（設定 → システム →「開発者向けオプション」。無ければ「端末情報」の「ビルド番号」を 7 回タップして出します）。</li>
      <li>スマホをこのパソコンに USB でつなぎ、スマホに出る「USB デバッグを許可しますか？」で「許可」を押します。{onWindows ? "（Windows では、スマホのメーカーの USB ドライバが要ることがあります。）" : ""}</li>
      <li>スマホの画面をつけたまま（ロックを解除したまま）にします。</li>
      <li>下の「スマホを確認」を押し、ぱち簿の入ったスマホが出たら「取り出す」を押します。取り出しの間はスマホを触らずに待ちます（画面が自動で動きます）。</li>
    </ol>
  </section>
)

/** What a read is waiting on the person for, while it waits. */
type WaitingNow = Exclude<Waiting, { readonly type: "back" }>

/** What to do while a read waits, in words. */
const waitingText = (now: WaitingNow): string => {
  switch (now.type) {
    case "unplugged":
      return `スマホとの接続が切れました。USB ケーブルをつなぎ直してください（あと ${String(now.seconds)} 秒ほど待ちます。つなぎ直したら、その月から読み直します）。`
    case "locked":
      return `スマホにロックがかかっています。ロックを解除してください（あと ${String(now.seconds)} 秒ほど待ちます）。`
  }
}

const phoneReady = (phones: Phones): boolean => phones.type === "ready" && phones.phone.hasPachibo

/** What the walk wrote, in words, with anything the person should look over. */
const Done = (props: { readonly rescued: Rescued; readonly expected: number | null; readonly out: string }): JSX.Element => {
  const short = (): boolean => props.expected !== null && props.rescued.count < props.expected
  return (
    <div class="done">
      <p>
        <Switch>
          <Match when={props.rescued.end.type === "cancelled"}>中断しました。それまでに読んだ</Match>
          <Match when={props.rescued.end.type === "failed"}>途中で止まりました。それまでに読んだ</Match>
        </Switch>
        収支 {count(props.rescued.count)} 件を書き出しました: {props.out}
      </p>
      <Show when={((end) => (end.type === "failed" ? end : undefined))(props.rescued.end)}>
        {(failed) => <p class="problem">{problemText(failed().problem)}</p>}
      </Show>
      <Show when={props.rescued.end.type === "finished" && short()}>
        <p class="problem">
          ぱち簿の件数（{count(props.expected ?? 0)} 件）より少なく読み取りました。読み取れなかった記録があるかもしれません。P-Lens に取り込む前に、ぱち簿と見比べてください。
        </p>
      </Show>
      <Show when={props.rescued.unsureMonths.length > 0}>
        <p class="problem">次の月は、読み取った収支の合計がぱち簿の月の合計と合いませんでした。ぱち簿と見比べてください: {props.rescued.unsureMonths.map(monthText).join("、")}</p>
      </Show>
      <p>P-Lens の「設定 → インポート → 汎用形式」で、このファイルを選び、列（日付・ホール・機種・投資・回収）を合わせてください。</p>
      <Show when={props.rescued.log}>{(log) => <p class="quiet">くわしい記録: {log()}</p>}</Show>
    </div>
  )
}

export const Android = (): JSX.Element => {
  const [phones, setPhones] = createSignal<Phones | undefined>(undefined)
  const [phonesProblem, setPhonesProblem] = createSignal<Problem | undefined>(undefined)
  const [checking, setChecking] = createSignal(false)
  const [rescue, setRescue] = createSignal<Rescue>({ type: "idle" })
  const [deep, setDeep] = createSignal(true)
  const [cancelled, setCancelled] = createSignal(false)
  const [waiting, setWaiting] = createSignal<WaitingNow | undefined>(undefined)

  const check = async (): Promise<void> => {
    setChecking(true)
    const result = await androidPhones()
    setPhones(result.ok ? result.value : undefined)
    setPhonesProblem(result.ok ? undefined : result.problem)
    setChecking(false)
  }

  onMount(() => void check())

  // The app is closing mid-reading: it stops the reading first (and closes once the phone is put back as it was).
  const closing = listen("android-closing", () => setCancelled(true))
  onCleanup(() => void closing.then((stop) => stop()))

  /** The survey and the walk, in order. Every failure — reported, or thrown where nothing was foreseen — ends as a problem shown, never as the screen left saying it is still reading. */
  const surveyAndWalk = async (serial: string, out: string): Promise<Rescue> => {
    setRescue({ type: "surveying" })
    const survey = await androidSurvey(serial)
    // Stopped while counting: no walk follows, whatever the count came to.
    if (cancelled()) return { type: "failed", problem: { type: "cancelled" } }
    if (!survey.ok) return { type: "failed", problem: survey.problem }

    const expected = survey.value.records
    const deepNow = deep()
    setRescue({ type: "rescuing", total: expected, perRecord: perRecordSeconds(deepNow), progress: undefined })
    const stopHearing = await onProgress((progress) => setRescue((state) => (state.type === "rescuing" ? { ...state, progress } : state)))
    try {
      const today = new Date()
      const result = await androidRescue(serial, out, deepNow, expected, today.getFullYear(), today.getMonth() + 1)
      return result.ok ? { type: "done", rescued: result.value, expected, out } : { type: "failed", problem: result.problem }
    } finally {
      stopHearing()
    }
  }

  const rescueFrom = async (serial: string): Promise<void> => {
    try {
      const out = chosenPath(await save({ title: "ぱち簿のデータの保存先", defaultPath: "pachibo.csv", filters: [{ name: "ぱち簿のデータ", extensions: ["csv"] }] }))
      if (out === undefined) return
      setCancelled(false)
      setWaiting(undefined)
      const stopHearing = await onWaiting((now) => setWaiting(now.type === "back" ? undefined : now))
      try {
        setRescue(await surveyAndWalk(serial, out))
      } finally {
        stopHearing()
        setWaiting(undefined)
      }
    } catch (thrown) {
      setRescue({ type: "failed", problem: problemOf(thrown) })
    }
  }

  const stopRescue = async (): Promise<void> => {
    setCancelled(true)
    const asked = await androidCancel()
    if (!asked.ok) {
      setCancelled(false)
      setRescue({ type: "failed", problem: asked.problem })
    }
  }

  const busy = (): boolean => ((state) => state === "surveying" || state === "rescuing")(rescue().type)

  return (
    <main>
      <Steps />
      <section class="backups">
        <header class="row">
          <h2>スマホ</h2>
          <span class="actions">
            <button type="button" onClick={() => void check()} disabled={checking() || busy()}>
              スマホを確認
            </button>
          </span>
        </header>

        <Switch>
          <Match when={checking()}>
            <p class="quiet">スマホを探しています…</p>
          </Match>
          <Match when={phonesProblem()}>{(problem) => <p class="problem">{problemText(problem())}</p>}</Match>
          <Match when={phones() === undefined}>
            <p class="quiet">スマホを探しています…</p>
          </Match>
          <Match when={phones()?.type === "no-adb"}>
            <p class="problem">スマホとやりとりする adb が見つかりません。アプリに同梱されているはずですが、見つからない場合は Android Platform Tools を入れて、環境変数 ADB にそのパスを指定してください。</p>
          </Match>
          <Match when={phones()?.type === "none"}>
            <p class="quiet">スマホが見つかりません。USB でつないで「USB デバッグ」をオンにし、「スマホを確認」をもう一度押してください。</p>
            <WindowsDriver />
          </Match>
          <Match when={phones()?.type === "unauthorized"}>
            <p class="problem">スマホがこのパソコンを許可していません。スマホの画面に出る「USB デバッグを許可しますか？」で「許可」を押してから、もう一度「スマホを確認」を押してください。</p>
          </Match>
          <Match when={((state) => (state?.type === "ready" ? state.phone : undefined))(phones())}>
            {(phone) => (
              <ul class="list">
                <li class="backup">
                  <div class="row">
                    <span class="device">
                      <strong>{phone().model || "スマホ"}</strong>
                      <span class="quiet">
                        {phone().hasPachibo ? "ぱち簿あり" : "ぱち簿が見つかりません"}
                        {phone().awake ? "" : "・画面がついていません"}
                      </span>
                    </span>
                    <button type="button" class="primary" disabled={!phoneReady(phones() as Phones) || busy()} onClick={() => void rescueFrom(phone().serial)}>
                      取り出す
                    </button>
                  </div>
                  <Show when={!phone().hasPachibo}>
                    <p class="quiet">このスマホにぱち簿が入っていません。ぱち簿の入ったスマホをつないでください。</p>
                  </Show>
                  <label class="password">
                    <input type="checkbox" checked={deep()} disabled={busy()} onChange={(event) => setDeep(event.currentTarget.checked)} />
                    <span>投資・回収・メモ・タグも取り出す（1 件ずつ開くので時間がかかります）</span>
                  </label>

                  <Switch>
                    <Match when={waiting()}>{(now) => <p class="problem">{waitingText(now())}</p>}</Match>
                    <Match when={rescue().type === "surveying"}>
                      <p class="quiet">取り出す件数を確認しています…（スマホの画面が動きます。触らずにお待ちください）</p>
                    </Match>
                    <Match when={((state) => (state.type === "rescuing" ? state : undefined))(rescue())}>
                      {(state) => (
                        <div class="rescuing">
                          <p class="quiet">
                            <Show
                              when={state().progress}
                              fallback={
                                <Show when={state().total} fallback={<>取り出しています… スマホは触らずにお待ちください。</>}>
                                  {(total) => (
                                    <>
                                      取り出しています… スマホは触らずにお待ちください。全 {count(total())} 件、{roughTime(total() * state().perRecord)}かかります。
                                    </>
                                  )}
                                </Show>
                              }
                            >
                              {(progress) => (
                                <>
                                  取り出しています… {count(progress().records)}
                                  <Show when={state().total}>{(total) => <> / {count(total())}</>}</Show> 件（{progress().year}年{progress().month}月
                                  <Show when={state().total}>{(total) => <>／のこり{roughTime(Math.max(0, total() - progress().records) * state().perRecord)}</>}</Show>
                                  ）。スマホは触らずにお待ちください。
                                </>
                              )}
                            </Show>
                          </p>
                        </div>
                      )}
                    </Match>
                    <Match when={((state) => (state.type === "done" ? state : undefined))(rescue())}>
                      {(done) => <Done rescued={done().rescued} expected={done().expected} out={done().out} />}
                    </Match>
                    <Match when={((state) => (state.type === "failed" ? state : undefined))(rescue())}>
                      {(failed) => <p class="problem">{problemText(failed().problem)}</p>}
                    </Match>
                  </Switch>
                  <Show when={busy()}>
                    <button type="button" onClick={() => void stopRescue()} disabled={cancelled()}>
                      {cancelled() ? "中断しています…" : "中断する"}
                    </button>
                  </Show>
                </li>
              </ul>
            )}
          </Match>
        </Switch>
      </section>
    </main>
  )
}
