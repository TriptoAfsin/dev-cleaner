import type { CacheItem } from "./api"

/** Which page lists a cache: System junk, a tab of AI / ML, or Disk cleanup. */
export type Section = "disk" | "junk" | "harnesses" | "mltools"
export const HARNESS_ECO = "AI harnesses"
export const ML_ECO = "AI / ML"
export const sectionOf = (c: CacheItem): Section => (c.junk ? "junk" : c.eco === HARNESS_ECO ? "harnesses" : c.eco === ML_ECO ? "mltools" : "disk")
