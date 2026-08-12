#!/usr/bin/env python3
"""Meridian watch — a terminal dashboard for the bot, viewed from Windows.

Pure monitoring: this end renders, the VPS-side collector gathers. One SSH
round trip per refresh brings back a single JSON blob, so a dropped
connection degrades to a stale-data banner instead of a crash, and nothing
here can affect the bot.

Run:  python tools/watch.py          (Ctrl+C or q to quit)
      python tools/watch.py --once   (single frame, for testing)
"""

import json
import subprocess
import sys
import time
from collections import deque
from datetime import datetime, timezone
from pathlib import Path

from rich.align import Align
from rich.console import Console, Group
from rich.layout import Layout
from rich.live import Live
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

SSH_TARGET = "vps3"
COLLECTOR = "python3 /root/meridian-build/tools/watch-collect.py"
REFRESH_S = 6
HISTORY_FILE = Path(__file__).with_name("watch-history.jsonl")
SPARK = "▁▂▃▄▅▆▇█"


def fetch():
    try:
        r = subprocess.run(
            ["ssh", "-o", "ConnectTimeout=8", "-o", "BatchMode=yes", SSH_TARGET, COLLECTOR],
            capture_output=True,
            text=True,
            timeout=25,
        )
        return json.loads(r.stdout)
    except Exception:
        return None


def load_history():
    hist = deque(maxlen=240)
    try:
        for line in HISTORY_FILE.read_text().splitlines()[-240:]:
            hist.append(json.loads(line))
    except OSError:
        pass
    return hist


def push_history(hist, total):
    hist.append({"t": datetime.now(timezone.utc).isoformat(), "v": total})
    try:
        with HISTORY_FILE.open("a") as f:
            f.write(json.dumps(hist[-1]) + "\n")
    except OSError:
        pass


def sparkline(values, width):
    if len(values) < 2:
        return "─" * width
    vals = values[-width:]
    lo, hi = min(vals), max(vals)
    if hi - lo < 1e-9:
        return "▄" * len(vals)
    return "".join(SPARK[int((v - lo) / (hi - lo) * (len(SPARK) - 1))] for v in vals)


def fmt_age(minutes):
    if minutes is None:
        return "-"
    return f"{minutes / 60:.1f}j" if minutes >= 60 else f"{minutes}m"


def fmt_uptime(s):
    return f"{s // 3600:02}:{s % 3600 // 60:02}:{s % 60:02}"


def pnl_text(pct, suffix="%"):
    if pct is None:
        return Text("-", style="dim")
    style = "green" if pct > 0 else "red" if pct < 0 else "dim"
    return Text(f"{pct:+.2f}{suffix}", style=style)


def bar(frac, width, style="green"):
    filled = max(0, min(width, round(frac * width)))
    t = Text("█" * filled, style=style)
    t.append("░" * (width - filled), style="grey30")
    return t


def build(d, hist, stale):
    lay = Layout()
    lay.split_column(
        Layout(name="head", size=1),
        Layout(name="profit", size=8),
        Layout(name="mid", ratio=2),
        Layout(name="log", size=11),
        Layout(name="foot", size=1),
    )
    lay["mid"].split_row(Layout(name="pos", ratio=3), Layout(name="side", ratio=2))
    # Proportional, not fixed: fixed heights on a short terminal cut one panel
    # mid-border and pushed the next off-screen entirely. Ratios shrink both
    # gracefully instead.
    lay["side"].split_column(
        Layout(name="risk", ratio=3),
        Layout(name="sys", ratio=2),
    )

    # ── header ───────────────────────────────────────────────────
    bot = d["bot"]
    live = bot["status"] == "online"
    head = Text()
    head.append(" ● ", style="bold green" if live else "bold red")
    head.append("LIVE " if live else f"{bot['status'].upper()} ", style="bold green" if live else "bold red")
    head.append(f" uptime {fmt_uptime(bot['uptime_s'])}", style="cyan")
    title = "M E R I D I A N   D L M M   B O T"
    if stale:
        title += "   [DATA BASI — koneksi putus]"
    head_l = Layout()
    head_l.split_row(
        Layout(Align.left(head)),
        Layout(Align.center(Text(title, style="bold yellow" if not stale else "bold red"))),
        Layout(Align.right(Text(datetime.now(timezone.utc).strftime("UTC %H:%M:%S "), style="cyan"))),
    )
    lay["head"].update(head_l)

    # ── profit ───────────────────────────────────────────────────
    w, base = d.get("wallet"), d.get("baseline")
    lines = []
    if w and base:
        net = w["total"] - base
        lines.append(Align.center(Text("─ SESSION PROFIT ─", style="yellow")))
        lines.append(
            Align.center(
                Text(
                    f"{net:+.4f} SOL",
                    style="bold green" if net >= 0 else "bold red",
                ).append(f"   ({w['total']:.4f} vs modal {base})", style="dim")
            )
        )
    lc = d.get("last_close")
    if lc:
        t = Text("last close ", style="dim")
        t.append(lc["pool"], style="bold")
        t.append(" ")
        t.append_text(pnl_text(lc["pnl_pct"]))
        t.append(f" ({fmt_age(lc['ago_min'])} lalu)", style="dim")
        lines.append(Align.center(t))
    win = d["window"]
    wr = win["wins"] * 100 // win["closes"] if win["closes"] else 0
    stats = Text()
    stats.append("24j ", style="dim")
    stats.append_text(pnl_text(win["pnl_sol"], " SOL"))
    stats.append("  ·  ", style="grey30")
    stats.append(f"{win['wins']}/{win['closes']} ", style="bold")
    stats.append(f"({wr}%)", style="green" if wr >= 60 else "yellow")
    stats.append("  ·  best ", style="dim")
    stats.append_text(pnl_text(win["best_sol"], ""))
    stats.append("  ·  worst ", style="dim")
    stats.append_text(pnl_text(win["worst_sol"], ""))
    lines.append(Align.center(stats))
    # The equity sparkline lives here, with the number it explains, instead of
    # in a side panel that kept falling off the bottom of short terminals.
    vals = [h["v"] for h in hist]
    if len(vals) >= 2:
        lines.append(Align.center(Text(sparkline(vals, 44), style="cyan")))
        lines.append(
            Align.center(
                Text(f"{vals[0]:.3f} → {vals[-1]:.3f} SOL", style="dim")
            )
        )
    lay["profit"].update(Panel(Group(*lines), border_style="yellow"))

    # ── positions ────────────────────────────────────────────────
    tbl = Table(expand=True, header_style="bold cyan", border_style="grey30")
    tbl.add_column("POOL")
    tbl.add_column("STATUS")
    tbl.add_column("PNL", justify="right")
    tbl.add_column("PEAK", justify="right")
    tbl.add_column("FEES $", justify="right")
    tbl.add_column("AGE", justify="right")
    for p in d["open"]:
        st = p["status"] or "-"
        tbl.add_row(
            Text(p["pool"], style="bold"),
            Text(st, style="green" if st == "active" else "yellow"),
            pnl_text(p["pnl_pct"]),
            pnl_text(p["peak"]),
            Text(f"{p['fees_usd']:.2f}" if p["fees_usd"] is not None else "-"),
            Text(fmt_age(p["age_min"]), style="dim"),
        )
    if not d["open"]:
        tbl.add_row(Text("(tidak ada posisi terbuka)", style="dim"), "", "", "", "", "")
    lay["pos"].update(Panel(tbl, title="LIVE POSITIONS", border_style="cyan"))

    # ── risk ─────────────────────────────────────────────────────
    risk = []
    if w:
        frac = w["locked"] / w["total"] if w["total"] else 0
        risk.append(Text(f"Exposure  {w['locked']:.2f} / {w['total']:.2f} SOL"))
        risk.append(bar(frac, 26))
        risk.append(Text(f"Slots {len(d['open'])}/{d.get('max_positions', '?')}   bebas {w['free']:.4f} SOL", style="dim"))
    if d["cooldowns"]:
        risk.append(Text(""))
        for c in d["cooldowns"][:3]:
            t = Text(f"⏳ {c['token'][:12]:<12} {c['left_min']:>4}m ", style="yellow")
            t.append(c["reason"], style="dim")
            risk.append(t)
    lay["risk"].update(Panel(Group(*risk), title="RISK", border_style="magenta"))

    # ── system ───────────────────────────────────────────────────
    h, dr = d["health"], d["drift"]
    sys_lines = [
        Text.assemble(("● " , "green" if live else "red"), f"pm2 {bot['status']}  (restart {bot['restarts']})"),
        Text.assemble(("● ", "green" if d["trading"] else "yellow"), "trading " + ("ON" if d["trading"] else "PAUSED")),
        Text.assemble(
            ("● ", "red" if h["err_429"] > 20 else "yellow" if h["err_429"] else "green"),
            f"RPC  429×{h['err_429']}  fail×{h['rpc_fail']}",
        ),
        Text(f"impact rejects (recent log): {h['impact_rejects']}", style="dim"),
    ]
    if dr["avg_pp"] is not None:
        sys_lines.append(Text(f"drift trigger→settle {dr['avg_pp']:+.2f}pp ({dr['n']})", style="dim"))
    lay["sys"].update(Panel(Group(*sys_lines), title="SYSTEM", border_style="green"))

    # ── event log ────────────────────────────────────────────────
    ev_lines = []
    for e in d["events"]:
        hhmm = e["t"][11:16]
        if e["kind"] == "open":
            t = Text(f" {hhmm} ", style="dim")
            t.append("▲ OPEN  ", style="cyan")
            t.append(e["pool"], style="bold")
        else:
            pnl = e.get("pnl_pct")
            mark = "✔" if (pnl or 0) > 0 else "✘" if (pnl or 0) < -1 else "·"
            t = Text(f" {hhmm} ", style="dim")
            t.append(f"{mark} CLOSE ", style="green" if (pnl or 0) > 0 else "red" if (pnl or 0) < 0 else "dim")
            t.append(f"{e['pool']:<14}", style="bold")
            t.append_text(pnl_text(pnl))
            t.append(f"  {e.get('reason', '')[:34]}", style="dim")
        ev_lines.append(t)
    if not ev_lines:
        ev_lines.append(Text(" (belum ada event 12 jam terakhir)", style="dim"))
    lay["log"].update(Panel(Group(*ev_lines), title="EVENT LOG", border_style="grey50"))

    lay["foot"].update(
        Align.center(Text(f"refresh {REFRESH_S}s · data dari vps3 · q keluar", style="dim"))
    )
    return lay


def main():
    # A legacy Windows console (or a pipe) defaults to cp1252, which cannot
    # encode the glyphs this dashboard is made of and kills the process with a
    # UnicodeEncodeError. Force UTF-8 with replacement so the worst case is an
    # ugly character, never a crash.
    if sys.platform == "win32":
        try:
            sys.stdout.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, OSError):
            pass

    once = "--once" in sys.argv
    console = Console()
    hist = load_history()
    data, stale = None, False

    if once:
        data = fetch()
        if not data:
            console.print("[red]gagal ambil data dari vps3[/red]")
            sys.exit(1)
        if data.get("wallet"):
            push_history(hist, data["wallet"]["total"])
        console.print(build(data, hist, False))
        return

    try:
        import msvcrt
    except ImportError:
        msvcrt = None

    with Live(console=console, screen=True, auto_refresh=False) as live:
        last_fetch = 0.0
        while True:
            if time.time() - last_fetch >= REFRESH_S:
                fresh = fetch()
                last_fetch = time.time()
                if fresh:
                    data, stale = fresh, False
                    if fresh.get("wallet"):
                        push_history(hist, fresh["wallet"]["total"])
                else:
                    stale = True
            if data:
                live.update(build(data, hist, stale), refresh=True)
            if msvcrt and msvcrt.kbhit() and msvcrt.getwch().lower() == "q":
                break
            time.sleep(1)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
