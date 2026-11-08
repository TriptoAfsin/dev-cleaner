import { useEffect, useRef, useState } from "react"
import { Camera, Copy, Download } from "lucide-react"
import { bytes } from "@/lib/format"
import { BrandIcon } from "@/lib/brands"
import { capture, copyPng, savePng, stamp } from "@/lib/snapshot"
import { useStore, type Report } from "@/lib/store"
import { AppMark } from "./TitleBar"
import { Button, Dialog, IconButton } from "./ui"

/** Camera button for a page header: snapshot the page, then copy or save. */
export function SnapshotButton({ name }: { name: string }) {
  const [shot, setShot] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const { toast } = useStore()
  const take = async () => {
    const page = document.querySelector<HTMLElement>("main [data-testid^='page-'] > div")
    if (!page) return
    setBusy(true)
    try {
      setShot(await capture(page))
    } catch (e) {
      toast({ kind: "error", title: "Could not capture the page", detail: String(e) })
    } finally {
      setBusy(false)
    }
  }
  return (
    <>
      <IconButton label="Save or copy as image" onClick={take} disabled={busy} data-testid="snapshot">
        <Camera size={14} className={busy ? "pulse-dot" : undefined} />
      </IconButton>
      <ImageDialog image={shot} name={`dev-cleaner-${name}-${stamp()}`} onClose={() => setShot(null)} title="Page snapshot" />
    </>
  )
}

function ImageDialog({ image, name, title, onClose }: { image: string | null; name: string; title: string; onClose: () => void }) {
  const { toast } = useStore()
  return (
    <Dialog
      open={!!image}
      onClose={onClose}
      title={title}
      actions={<ShareActions getImage={async () => image!} name={name} onDone={onClose} toast={toast} />}
    >
      {image && <img src={image} alt="Snapshot preview" className="w-full rounded-lg border border-border" />}
    </Dialog>
  )
}

function ShareActions({ getImage, name, onDone, toast }: { getImage: () => Promise<string>; name: string; onDone: () => void; toast: ReturnType<typeof useStore>["toast"] }) {
  const [busy, setBusy] = useState<"copy" | "save" | null>(null)
  return (
    <>
      <Button
        icon={<Copy size={14} />}
        loading={busy === "copy"}
        onClick={async () => {
          setBusy("copy")
          try {
            await copyPng(await getImage())
            toast({ kind: "ok", title: "Image copied", detail: "Paste it into a chat or post." })
            onDone()
          } catch (e) {
            toast({ kind: "error", title: "Copy failed", detail: String(e) })
          } finally {
            setBusy(null)
          }
        }}
      >
        Copy image
      </Button>
      <Button
        variant="primary"
        icon={<Download size={14} />}
        loading={busy === "save"}
        data-testid="save-png"
        onClick={async () => {
          setBusy("save")
          try {
            const path = await savePng(await getImage(), name)
            if (path) {
              toast({ kind: "ok", title: "Image saved", detail: path })
              onDone()
            }
          } catch (e) {
            toast({ kind: "error", title: "Save failed", detail: String(e) })
          } finally {
            setBusy(null)
          }
        }}
      >
        Save PNG
      </Button>
    </>
  )
}

/** After a clean: a card worth sharing, rendered live and exported as PNG. */
export function ShareReportDialog() {
  const { report, closeReport, toast } = useStore()
  const card = useRef<HTMLDivElement>(null)
  const [open, setOpen] = useState(false)
  useEffect(() => setOpen(!!report), [report])
  return (
    <Dialog
      open={open}
      onClose={closeReport}
      title="Share what you freed"
      actions={<ShareActions getImage={() => capture(card.current!)} name={`dev-cleaner-freed-${stamp()}`} onDone={closeReport} toast={toast} />}
    >
      {report && (
        <div className="overflow-hidden rounded-xl border border-border">
          <ReportCard ref={card} report={report} />
        </div>
      )}
    </Dialog>
  )
}

function ReportCard({ report, ref }: { report: Report; ref: React.Ref<HTMLDivElement> }) {
  const items = [...report.items].filter((i) => i.freed > 0).sort((a, b) => b.freed - a.freed)
  const top = items.slice(0, 6)
  const max = Math.max(1, top[0]?.freed ?? 1)
  return (
    <div ref={ref} className="flex w-[420px] max-w-full flex-col gap-4 bg-surface p-6 text-text" data-testid="report-card">
      <div className="flex items-center gap-2">
        <AppMark size={18} />
        <span className="text-[13px] font-semibold">Dev Cleaner</span>
        <span className="ml-auto text-[11.5px] text-subtle">{new Date(report.at).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}</span>
      </div>
      <div>
        <div className="text-[12px] text-muted">{report.title}</div>
        <div className="mono text-[44px] font-semibold leading-none tracking-[-0.03em] text-accent">{bytes(report.total)}</div>
        <div className="mt-1.5 text-[12.5px] text-muted">
          freed from {items.length} {report.kind === "projects" ? "build folders" : "caches"}
        </div>
      </div>
      <ul className="m-0 flex list-none flex-col gap-2 p-0">
        {top.map((i) => (
          <li key={i.name} className="grid grid-cols-[16px_minmax(0,1fr)_64px] items-center gap-2.5 text-[12.5px]">
            <BrandIcon name={i.icon} size={14} />
            <div className="min-w-0">
              <div className="truncate">{i.name}</div>
              <div className="mt-1 h-1 overflow-hidden rounded-full bg-track">
                <div className="h-full rounded-full bg-accent" style={{ width: `${(i.freed / max) * 100}%` }} />
              </div>
            </div>
            <span className="mono text-right">{bytes(i.freed)}</span>
          </li>
        ))}
        {items.length > top.length && <li className="text-[11.5px] text-subtle">and {items.length - top.length} more</li>}
      </ul>
      <div className="border-t border-border pt-3 text-[11px] text-subtle">github.com/TriptoAfsin/dev-cleaner</div>
    </div>
  )
}

export { Camera }
