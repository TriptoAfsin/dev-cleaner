import { useMemo, useState } from "react"
import { MemoryStick, RefreshCw, Trash2 } from "lucide-react"
import { api } from "@/lib/api"
import { bytes } from "@/lib/format"
import { usePoll, useStore } from "@/lib/store"
import { AnimatedNumber, Button, Card, Gauge, PageHeader, Skeleton, Tooltip } from "@/components/ui"
import { CacheRow, TopCpu, useKill } from "@/components/shared"
import type { Page } from "@/components/Shell"
import { freeMemoryHelp, useFreeMemory } from "./Memory"
import { InsightsCard } from "@/components/Insights"
import { SnapshotButton } from "@/components/Share"
import { insights } from "@/lib/insights"
import type { SystemInfo } from "@/lib/api"
import { CleanDialog, preselect } from "./Disk"

export function Overview({ go }: { go: (p: Page) => void }) {
  const { stats, caches, scanning, scan, lastScan, platform } = useStore()
  const procs = usePoll(api.listProcesses, 2500)
  const sys = usePoll(api.systemInfo, 120_000)
  const kill = useKill(procs.refresh)
  const free = useFreeMemory()
  const reclaim = caches.reduce((n, c) => n + c.size, 0)
  const top = useMemo(() => caches.filter((c) => !c.junk && c.size > 0).slice(0, 5), [caches])
  const [sel, setSel] = useState<Set<string> | null>(null)
  // Everything safe and found is selected until the user changes it.
  const selected = sel ?? new Set(top.filter(preselect).map((c) => c.id))
  const selBytes = top.filter((c) => selected.has(c.id)).reduce((n, c) => n + c.size, 0)
  const [confirm, setConfirm] = useState(false)

  const disk = stats?.disk
  const diskUsed = disk ? 1 - disk.free / disk.total : 0
  const memUsed = stats ? stats.memUsed / stats.memTotal : 0

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Overview"
        lede={
          scanning && !reclaim ? (
            "Measuring caches…"
          ) : (
            <>
              <AnimatedNumber value={reclaim} format={(n) => bytes(n)} /> of caches can be rebuilt on demand. Nothing is removed until you confirm.
            </>
          )
        }
        actions={
          <>
            <SnapshotButton name="overview" />
            <Tooltip text={freeMemoryHelp(platform)} wide side="bottom">
              <Button icon={<MemoryStick size={15} />} loading={free.busy} onClick={free.run} data-testid="free-memory">
                Free memory
              </Button>
            </Tooltip>
            <Button variant="primary" icon={<RefreshCw size={15} className={scanning ? "spin" : undefined} />} disabled={scanning} onClick={() => void scan()} data-testid="scan">
              {scanning ? "Scanning" : "Scan"}
            </Button>
          </>
        }
      />

      <div className="grid grid-cols-3 gap-3.5">
        <GaugeCard label="CPU" value={(stats?.cpu ?? 0) / 100} detail={stats ? `${stats.cores.length} threads` : null} note={stats ? ((stats.cpu ?? 0) > 80 ? "Busy" : "Normal load") : null} warn={(stats?.cpu ?? 0) > 80} />
        <GaugeCard
          label="Memory"
          value={memUsed}
          detail={stats ? `${bytes(stats.memUsed)} of ${bytes(stats.memTotal, 0)}` : null}
          note={stats ? (stats.compressed ? `${bytes(stats.compressed)} compressed` : `${bytes(stats.memAvailable)} available`) : null}
          warn={memUsed > 0.85}
        />
        <GaugeCard
          label={disk ? `Disk ${disk.mount}` : "Disk"}
          value={diskUsed}
          detail={disk ? `${bytes(disk.total - disk.free, 0)} of ${bytes(disk.total, 0)}` : null}
          note={disk ? (diskUsed > 0.85 ? `Running low · ${bytes(disk.free, 0)} free` : `${bytes(disk.free, 0)} free`) : null}
          warn={diskUsed > 0.85}
        />
      </div>

      <div className="grid grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)] gap-3.5">
        <Card className="flex flex-col overflow-hidden">
          <div className="flex items-center justify-between px-4 py-3">
            <h2 className="m-0 text-[13px] font-semibold">Reclaimable caches</h2>
            <button type="button" className="text-[12px] text-accent hover:underline" onClick={() => go("disk")}>
              Review all
            </button>
          </div>
          <ul className="m-0 list-none p-0">
            {top.length === 0 && scanning
              ? Array.from({ length: 5 }).map((_, i) => (
                  <li key={i} className="border-t border-border px-4 py-3">
                    <Skeleton className="h-4" />
                  </li>
                ))
              : top.map((c) => (
                  <CacheRow
                    key={c.id}
                    c={c}
                    max={top[0]?.size ?? 1}
                    selected={selected.has(c.id)}
                    onToggle={(v) => {
                      const s = new Set(selected)
                      if (v) s.add(c.id)
                      else s.delete(c.id)
                      setSel(s)
                    }}
                  />
                ))}
          </ul>
          <div className="mt-auto flex items-center justify-between border-t border-border bg-surface-2 px-4 py-2.5">
            <span className="mono text-[12.5px]">
              {selected.size} selected · <AnimatedNumber value={selBytes} format={(n) => bytes(n)} />
            </span>
            <Button variant="primary" size="sm" className="h-[30px] px-3" icon={<Trash2 size={14} />} disabled={!selected.size || scanning} onClick={() => setConfirm(true)} data-testid="clean-selected">
              Clean selected
            </Button>
          </div>
          {lastScan == null && !scanning && <div className="px-4 py-3 text-[12px] text-subtle">Not scanned yet.</div>}
        </Card>

        <Card className="flex flex-col overflow-hidden">
          <div className="flex items-center justify-between px-4 py-3">
            <h2 className="m-0 text-[13px] font-semibold">Top CPU</h2>
            <span className="mono text-[11.5px] text-subtle">live</span>
          </div>
          <TopCpu procs={procs.data} onKill={(g) => kill.request(g.name, g.pids, g)} />
        </Card>
      </div>
      <InsightsCard items={insights(sys.data as SystemInfo | null, stats, caches, procs.data)} go={go} limit={3} loading={!sys.data} />
      {kill.dialog}
      {free.dialog}
      <CleanDialog open={confirm} onClose={() => setConfirm(false)} items={top.filter((c) => selected.has(c.id))} />
    </div>
  )
}

function GaugeCard({ label, value, detail, note, warn }: { label: string; value: number; detail: string | null; note: string | null; warn?: boolean }) {
  return (
    <Card className="flex items-center gap-4 px-4 py-3.5" data-testid={`gauge-${label.split(" ")[0].toLowerCase()}`}>
      <Gauge value={value} warn={warn}>
        <span className="mono text-[17px] font-medium">{detail == null ? "—" : `${Math.round(value * 100)}%`}</span>
      </Gauge>
      <div className="min-w-0">
        <div className="text-[13px] font-semibold">{label}</div>
        {detail == null ? <Skeleton className="mt-1.5 h-3.5 w-28" /> : <div className="mono mt-0.5 truncate text-[12px] text-muted">{detail}</div>}
        {note && <div className={warn ? "mt-1.5 text-[11.5px] text-warn" : "mt-1.5 text-[11.5px] text-subtle"}>{note}</div>}
      </div>
    </Card>
  )
}
