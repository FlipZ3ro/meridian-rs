#!/usr/bin/env python3
"""Collector for the Meridian watch dashboard.

Runs on the VPS next to the bot and gathers everything the Windows-side
renderer needs into a single JSON blob on stdout: one SSH round trip per
refresh instead of six. The renderer stays dumb on purpose — every derived
number (win rate, exposure, event log) is computed here, where the data is.
"""

import json
import os
import re
import subprocess
import sys
import urllib.request
from datetime import datetime, timedelta, timezone

STATE = "/root/.meridian-tele/meridian-state.json"
POOL_MEMORY = "/root/.meridian-tele/pool-memory.json"
TRADING_FLAG = "/root/.meridian-tele/trading-enabled.json"
ENV_FILE = "/root/meridian-build/backend/.env"
CONFIG = "/root/meridian-build/backend/user-config.json"
LOG_OUT = "/root/.pm2/logs/meridian-tele-out.log"
LOG_ERR = "/root/.pm2/logs/meridian-tele-error.log"
WALLET = "3k7tWvC9ZnfFjASwb8zJn43E4pF1oi68sXX1cy2hBEGq"
# All-in cost of one position: 0.5 liquidity + ~0.0574 rent, measured as the
# median of 104 deploy transactions.
POSITION_COST = 0.5574

now = datetime.now(timezone.utc)


def ts(x):
    try:
        return datetime.fromisoformat(x.replace("Z", "+00:00"))
    except (ValueError, AttributeError, TypeError):
        return None


def load_json(path, default):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError):
        return default


def tail(path, lines):
    try:
        r = subprocess.run(
            ["tail", "-n", str(lines), path], capture_output=True, text=True, timeout=5
        )
        return r.stdout
    except Exception:
        return ""


out = {"ts": now.isoformat(), "ok": True}

# ── bot process ──────────────────────────────────────────────────
out["bot"] = {"status": "unknown", "uptime_s": 0, "restarts": 0}
try:
    r = subprocess.run(["pm2", "jlist"], capture_output=True, text=True, timeout=10)
    for p in json.loads(r.stdout or "[]"):
        if p.get("name") == "meridian-tele":
            env = p.get("pm2_env", {})
            out["bot"] = {
                "status": env.get("status", "unknown"),
                "uptime_s": max(0, int(now.timestamp() - env.get("pm_uptime", 0) / 1000)),
                "restarts": env.get("restart_time", 0),
            }
except Exception:
    pass

try:
    with open(TRADING_FLAG) as f:
        out["trading"] = f.read().strip() != "false"
except OSError:
    out["trading"] = True

cfg = load_json(CONFIG, {})
out["baseline"] = cfg.get("management", {}).get("baselineSol")
out["max_positions"] = cfg.get("risk", {}).get("maxPositions")

# ── positions ────────────────────────────────────────────────────
state = load_json(STATE, {"positions": {}})
positions = list(state.get("positions", {}).values())
open_pos = [p for p in positions if p.get("status") != "closed"]
closed = [p for p in positions if p.get("status") == "closed" and p.get("closed_at")]

out["open"] = [
    {
        "pool": p.get("pool_name") or "?",
        "status": p.get("status"),
        "pnl_pct": p.get("pnl_pct"),
        "pnl_sol": p.get("pnl_sol"),
        "peak": (p.get("trailing") or {}).get("peak_pnl_pct"),
        "fees_usd": p.get("all_time_fees_usd"),
        "age_min": int((now - t).total_seconds() / 60)
        if (t := ts(p.get("created_at")))
        else None,
    }
    for p in sorted(open_pos, key=lambda p: p.get("created_at") or "")
]

cut = now - timedelta(hours=24)
c24 = [p for p in closed if (t := ts(p["closed_at"])) and t >= cut]
wins = [p for p in c24 if (p.get("pnl_sol") or 0) > 0]
sols = [(p.get("pnl_sol") or 0.0) for p in c24]
out["window"] = {
    "closes": len(c24),
    "wins": len(wins),
    "pnl_sol": sum(sols),
    "best_sol": max(sols) if sols else 0.0,
    "worst_sol": min(sols) if sols else 0.0,
}

last_close = max(closed, key=lambda p: p["closed_at"], default=None)
out["last_close"] = (
    {
        "pool": last_close.get("pool_name") or "?",
        "pnl_pct": last_close.get("pnl_pct"),
        "pnl_sol": last_close.get("pnl_sol"),
        "ago_min": int((now - t).total_seconds() / 60)
        if (t := ts(last_close["closed_at"]))
        else None,
    }
    if last_close
    else None
)

drifts = [
    p["settled_pnl_pct"] - p["pnl_pct"]
    for p in positions
    if p.get("settled_pnl_pct") is not None and p.get("pnl_pct") is not None
]
out["drift"] = {
    "avg_pp": sum(drifts) / len(drifts) if drifts else None,
    "n": len(drifts),
}

# ── event log: opens and closes interleaved, newest first ────────
events = []
for p in positions:
    if (t := ts(p.get("created_at"))) and t >= now - timedelta(hours=12):
        events.append(
            {"t": p["created_at"], "kind": "open", "pool": p.get("pool_name") or "?"}
        )
    if p.get("status") == "closed" and (t := ts(p.get("closed_at"))) and t >= now - timedelta(hours=12):
        reason = (p.get("close_reason") or "").replace("auto-close (pnl_poll): ", "")
        events.append(
            {
                "t": p["closed_at"],
                "kind": "close",
                "pool": p.get("pool_name") or "?",
                "pnl_pct": p.get("pnl_pct"),
                "pnl_sol": p.get("pnl_sol"),
                "reason": reason[:44],
            }
        )
events.sort(key=lambda e: e["t"], reverse=True)
out["events"] = events[:8]

# ── cooldowns ────────────────────────────────────────────────────
pm = load_json(POOL_MEMORY, {"pools": {}})
cds, seen = [], set()
for e in pm.get("pools", {}).values():
    until = e.get("base_mint_cooldown_until") or e.get("cooldown_until")
    if not until or not (t := ts(until)) or t <= now:
        continue
    name = (e.get("name") or "?").rsplit("-", 1)[0]
    if name in seen:
        continue
    seen.add(name)
    cds.append(
        {
            "token": name,
            "left_min": int((t - now).total_seconds() / 60),
            "reason": (e.get("base_mint_cooldown_reason") or e.get("cooldown_reason") or "")[:28],
        }
    )
cds.sort(key=lambda c: -c["left_min"])
out["cooldowns"] = cds[:6]

# ── wallet ───────────────────────────────────────────────────────
out["wallet"] = None
try:
    rpc_url = None
    with open(ENV_FILE) as f:
        for line in f:
            if line.startswith("HELIUS_RPC_URL="):
                rpc_url = line.split("=", 1)[1].strip().strip('"')
                break
    if rpc_url:
        req = urllib.request.Request(
            rpc_url,
            data=json.dumps(
                {"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [WALLET]}
            ).encode(),
            headers={"content-type": "application/json"},
        )
        free = json.loads(urllib.request.urlopen(req, timeout=8).read())["result"][
            "value"
        ] / 1e9
        n = len(open_pos)
        unreal = sum((p.get("pnl_sol") or 0.0) for p in open_pos)
        out["wallet"] = {
            "free": free,
            "locked": n * POSITION_COST,
            "unreal": unreal,
            "total": free + n * POSITION_COST + unreal,
        }
except Exception:
    pass

# ── log health ───────────────────────────────────────────────────
recent = tail(LOG_OUT, 400) + tail(LOG_ERR, 200)
out["health"] = {
    # Count the literal rate-limit message, not the bare substring: "429"
    # matches inside timestamps and signatures (T09:42:18.484291Z), which made
    # a healthy RPC read as throttled on the dashboard.
    "err_429": recent.count("Too Many Requests") + recent.count("max usage"),
    "rpc_fail": len(re.findall(r"health check failed", recent)),
    "errors": len(re.findall(r"ERROR", recent)),
    "impact_rejects": len(re.findall(r"exit price impact .* above max", recent)),
}

json.dump(out, sys.stdout)
