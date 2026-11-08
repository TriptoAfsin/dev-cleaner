// Update checks against GitHub releases, shared by the title-bar About
// button and the Settings card. One check runs at a time.
import { useSyncExternalStore } from "react"
import { api, type UpdateInfo } from "./api"

export type UpdateState = {
  status: "idle" | "checking" | "done" | "error"
  info?: UpdateInfo
  error?: string
  checkedAt?: number
}

let state: UpdateState = { status: "idle" }
const subs = new Set<() => void>()
const emit = (s: UpdateState) => {
  state = s
  subs.forEach((f) => f())
}

export const LAST_CHECK_KEY = "dc-update-checked"

export async function checkForUpdates(): Promise<UpdateState> {
  if (state.status === "checking") return state
  emit({ ...state, status: "checking", error: undefined })
  try {
    const info = await api.checkUpdate()
    emit({ status: "done", info, checkedAt: Date.now() })
  } catch (e) {
    emit({ status: "error", error: String(e), checkedAt: Date.now() })
  }
  try {
    localStorage.setItem(LAST_CHECK_KEY, String(Date.now()))
  } catch {
    /* storage unavailable */
  }
  return state
}

export function useUpdates(): UpdateState {
  return useSyncExternalStore(
    (f) => {
      subs.add(f)
      return () => subs.delete(f)
    },
    () => state,
  )
}
