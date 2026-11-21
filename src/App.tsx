import { open, save } from "@tauri-apps/plugin-dialog"
import { createSignal, For, Match, onMount, Show, Switch, type JSX } from "solid-js"

import { Android } from "./Android"
import { backupsInFolder, findBackups, hasUsualPlace, rescue, type BackupView, type Problem, type Result } from "./backups"
import { deviceText, iosText, madeAtText, needsFullDiskAccess, problemText } from "./wording"

/** Which phone the person is taking records off: an iPhone backup, or an Android phone over USB. */
type Tab = "iphone" | "android"

/** Where the list was last read from: the usual place, a folder the person chose, or nowhere yet where there is no usual place. */
type Place = { readonly type: "usual" } | { readonly type: "folder"; readonly path: string } | { readonly type: "unchosen" }

type Listing =
  | { readonly type: "unchosen" }
  | { readonly type: "looking" }
  | { readonly type: "listed"; readonly backups: readonly BackupView[] }
  | { readonly type: "failed"; readonly problem: Problem }

type Rescue =
  | { readonly type: "idle" }
  | { readonly type: "rescuing"; readonly path: string }
  | { readonly type: "rescued"; readonly path: string; readonly tallies: number; readonly out: string }
  | { readonly type: "failed"; readonly path: string; readonly problem: Problem }

const listingOf = (result: Result<readonly BackupView[]>): Listing => (result.ok ? { type: "listed", backups: result.value } : { type: "failed", problem: result.problem })

const chosenPath = (chosen: string | string[] | null): string | undefined => (typeof chosen === "string" ? chosen : undefined)

const Intro = (): JSX.Element => (
  <section class="intro">
    <h1>ぱち簿のデータを取り出す</h1>
    <p>iPhone のバックアップからぱち簿のデータを取り出して、P-Lens の「設定 → インポート → ぱち簿」で読み込めるファイル（.pcb）にします。iPhone にもどこにもデータを送りません。</p>
    <ol>
      <li>iPhone をこのパソコンにつなぎ、Mac は Finder、Windows は Apple Devices（または iTunes）で「今すぐバックアップ」を押します。</li>
      <li>下の一覧に出てきたバックアップの「取り出す」を押して、保存先を選びます。</li>
    </ol>
  </section>
)

const FullDiskAccess = (): JSX.Element => (
  <section class="notice">
    <h2>フルディスクアクセスの許可が必要です</h2>
    <p>Mac では、バックアップのフォルダを読むのに許可が要ります。</p>
    <ol>
      <li>システム設定 →「プライバシーとセキュリティ」→「フルディスクアクセス」を開きます。</li>
      <li>一覧の pachibo-rescue をオンにします（無ければ「＋」から アプリケーション の pachibo-rescue を加えます）。</li>
      <li>このアプリを一度終了して、開き直します。</li>
    </ol>
  </section>
)

const Iphone = (): JSX.Element => {
  const [place, setPlace] = createSignal<Place>({ type: "usual" })
  const [listing, setListing] = createSignal<Listing>({ type: "looking" })
  const [rescueState, setRescue] = createSignal<Rescue>({ type: "idle" })
  const [passwords, setPasswords] = createSignal<ReadonlyMap<string, string>>(new Map())

  const lookAt = async (at: Place): Promise<void> => {
    setPlace(at)
    if (at.type === "unchosen") {
      setListing({ type: "unchosen" })
      return
    }
    setListing({ type: "looking" })
    setListing(listingOf(await (at.type === "usual" ? findBackups() : backupsInFolder(at.path))))
  }

  const lookInFolder = async (): Promise<void> => {
    const folder = chosenPath(await open({ directory: true, title: "バックアップのフォルダを選ぶ" }))
    if (folder !== undefined) await lookAt({ type: "folder", path: folder })
  }

  const setPassword = (path: string, password: string): void => {
    setPasswords((held) => new Map([...held, [path, password]]))
  }

  const rescueFrom = async (backup: BackupView): Promise<void> => {
    const out = chosenPath(await save({ title: "ぱち簿のデータの保存先", defaultPath: "pachibo.pcb", filters: [{ name: "ぱち簿のデータ", extensions: ["pcb"] }] }))
    if (out === undefined) return
    setRescue({ type: "rescuing", path: backup.path })
    const result = await rescue(backup.path, backup.encrypted ? passwords().get(backup.path) : undefined, out)
    setRescue(result.ok ? { type: "rescued", path: backup.path, tallies: result.value, out } : { type: "failed", path: backup.path, problem: result.problem })
  }

  const rescueOf = (path: string): Rescue => ((state) => (state.type !== "idle" && state.path === path ? state : { type: "idle" }))(rescueState())

  onMount(() => void hasUsualPlace().then((usual) => lookAt(usual ? { type: "usual" } : { type: "unchosen" })))

  return (
    <main>
      <Intro />
      <section class="backups">
        <header class="row">
          <h2>バックアップ</h2>
          <span class="actions">
            <Show when={place().type !== "unchosen"}>
              <button type="button" onClick={() => void lookAt(place())}>
                一覧を更新
              </button>
            </Show>
            <button type="button" onClick={() => void lookInFolder()}>
              フォルダを選ぶ
            </button>
          </span>
        </header>
        <Switch>
          <Match when={listing().type === "unchosen"}>
            <p class="quiet">「フォルダを選ぶ」で、バックアップのあるフォルダを選んでください。</p>
          </Match>
          <Match when={listing().type === "looking"}>
            <p class="quiet">探しています…</p>
          </Match>
          <Match when={((state) => (state.type === "failed" ? state : undefined))(listing())}>
            {(failed) => (
              <>
                <Show when={needsFullDiskAccess(failed().problem)}>
                  <FullDiskAccess />
                </Show>
                <p class="problem">{problemText(failed().problem)}</p>
              </>
            )}
          </Match>
          <Match when={((state) => (state.type === "listed" ? state : undefined))(listing())}>
            {(listed) => (
              <Show when={listed().backups.length > 0} fallback={<p class="quiet">バックアップが見つかりません。iPhone のバックアップを作ってから「一覧を更新」を押すか、「フォルダを選ぶ」でバックアップのあるフォルダを選んでください。</p>}>
                <ul class="list">
                  <For each={listed().backups}>
                    {(backup) => (
                      <li class="backup">
                        <div class="row">
                          <span class="device">
                            <strong>{deviceText(backup)}</strong>
                            <span class="quiet">
                              {madeAtText(backup.madeAt)}・{iosText(backup)}
                              {backup.encrypted ? "・暗号化" : ""}
                            </span>
                          </span>
                          <button type="button" class="primary" disabled={!backup.hasPachibo || rescueOf(backup.path).type === "rescuing"} onClick={() => void rescueFrom(backup)}>
                            取り出す
                          </button>
                        </div>
                        <Show when={!backup.hasPachibo}>
                          <p class="quiet">ぱち簿が入っていないバックアップです。</p>
                        </Show>
                        <Show when={backup.encrypted && backup.hasPachibo}>
                          <label class="password">
                            <span>バックアップのパスワード</span>
                            <input type="password" autocomplete="off" value={passwords().get(backup.path) ?? ""} onInput={(event) => setPassword(backup.path, event.currentTarget.value)} />
                          </label>
                        </Show>
                        <Switch>
                          <Match when={rescueOf(backup.path).type === "rescuing"}>
                            <p class="quiet">取り出しています…（暗号化されたバックアップは数十秒かかります）</p>
                          </Match>
                          <Match when={((state) => (state.type === "rescued" ? state : undefined))(rescueOf(backup.path))}>
                            {(rescued) => (
                              <p class="done">
                                収支 {rescued().tallies.toLocaleString("ja-JP")} 件を書き出しました: {rescued().out}
                                <br />
                                P-Lens の「設定 → インポート → ぱち簿」で、このファイルを選んでください。
                              </p>
                            )}
                          </Match>
                          <Match when={((state) => (state.type === "failed" ? state : undefined))(rescueOf(backup.path))}>
                            {(failed) => <p class="problem">{problemText(failed().problem)}</p>}
                          </Match>
                        </Switch>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            )}
          </Match>
        </Switch>
      </section>
    </main>
  )
}

/** Both tabs stay mounted once opened, only hidden when the other is shown, so a reading under way keeps its progress and its stop button across a switch. Android is mounted on its first opening, so adb is not started for a person who only uses iPhone. */
export const App = (): JSX.Element => {
  const [tab, setTab] = createSignal<Tab>("iphone")
  const [androidOpened, setAndroidOpened] = createSignal(false)
  const show = (to: Tab): void => {
    if (to === "android") setAndroidOpened(true)
    setTab(to)
  }
  return (
    <>
      <nav class="tabs">
        <button type="button" classList={{ on: tab() === "iphone" }} onClick={() => show("iphone")}>
          iPhone
        </button>
        <button type="button" classList={{ on: tab() === "android" }} onClick={() => show("android")}>
          Android
        </button>
      </nav>
      <div class="tab" hidden={tab() !== "iphone"}>
        <Iphone />
      </div>
      <Show when={androidOpened()}>
        <div class="tab" hidden={tab() !== "android"}>
          <Android />
        </div>
      </Show>
    </>
  )
}
