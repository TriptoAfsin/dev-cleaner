import type { Page } from "@/components/Shell"
import { Disk } from "./Disk"

/** System junk: Quick clean (the junk caches) and Deep clean (stub). */
export function Junk(_: { go: (p: Page) => void }) {
  return <Disk section="junk" key="junk" />
}
