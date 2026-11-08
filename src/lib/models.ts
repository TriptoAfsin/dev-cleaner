import { invoke } from "@tauri-apps/api/core"

export type Model = {
  id: string
  name: string
  /** "ollama", "huggingface", "hf-dataset", "lmstudio", "comfyui", "a1111", "jan", "gpt4all", "whisper", "modelscope", "llamacpp", "torch", "loose" */
  kind: string
  /** Human label, e.g. "ComfyUI · checkpoints" or "Loose file". */
  source: string
  format: string
  path: string
  size: number
  files: number
  modified: number
  accessed: number | null
  deletable: boolean
  note: string
}

export type ModelDeleteEvent = { id: string; ok: boolean; freed: number; error: string | null }

export const modelsApi = {
  scan: (extraRoots: string[]) => invoke<Model[]>("scan_models", { extraRoots }),
  cancel: () => invoke<void>("cancel_model_scan"),
  remove: (ids: string[]) => invoke<ModelDeleteEvent[]>("delete_models", { ids }),
}

const ROOTS_KEY = "dc-model-roots"

export function loadModelRoots(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(ROOTS_KEY) ?? "[]")
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []
  } catch {
    return []
  }
}

export function saveModelRoots(roots: string[]) {
  try {
    localStorage.setItem(ROOTS_KEY, JSON.stringify(roots))
  } catch {
    /* storage unavailable; the list just won't persist */
  }
}

/** Quantization tag visible in a model name: Q4_K_M, IQ3_XS, Q8_0, F16, BF16, 4bit, int8... */
export function quantOf(name: string): string | null {
  const m = name.match(/(?:^|[^A-Za-z0-9])(I?Q\d(?:_[A-Z0-9]+)*|BF16|FP16|F16|F32|FP8|INT[48]|[48]-?bit)(?=$|[^A-Za-z0-9])/i)
  return m ? m[1].toUpperCase() : null
}

/** Seconds since epoch it was last read, falling back to when it changed. */
export function lastUsed(m: Model): number {
  return Math.max(m.accessed ?? 0, m.modified)
}
