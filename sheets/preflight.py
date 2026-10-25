"""Preflight: every row x column crossing is a checkbox.

Lists unfilled cells (null), rows not yet verified, and cross-sheet references that do not resolve.
Exit code 0 only when the build-relevant cells are all filled and every reference resolves.
Usage: python sheets/preflight.py [--strict]   (--strict also requires verified=true everywhere)
"""
import json, os, sys, glob

HERE = os.path.dirname(os.path.abspath(__file__))
sheets = {}
for p in sorted(glob.glob(os.path.join(HERE, "*.json"))):
    s = json.load(open(p, encoding="utf-8"))
    sheets[s["sheet"]] = s

ids = {name: {r["id"] for r in s["rows"]} for name, s in sheets.items()}

# column -> sheet it must reference (value or list of values)
REFS = {
    ("fighters", "abilities"): "abilities",
    ("fighters", "stat_profile"): "stat_profiles",
    ("abilities", "owner"): "fighters",
    ("abilities", "delivery"): "deliveries",
    ("abilities", "jf_sfx"): "audio",
    ("deliveries", "hook"): "hooks",
    ("coop_messages", "hook"): "hooks",
    ("audio", "fighter"): "fighters",
}
SPECIAL = {"$loadout", "$player", "avatar", "none"}

unfilled, broken, unverified = [], [], []
for name, s in sheets.items():
    cols = list(s["columns"].keys())
    for r in s["rows"]:
        for c in cols:
            if c not in r:
                unfilled.append(f"{name}.{r['id']}.{c} (missing)")
                continue
            v = r[c]
            if c == "verified":
                if v is not True:
                    unverified.append(f"{name}.{r['id']}")
                continue
            if v is None:
                unfilled.append(f"{name}.{r['id']}.{c}")
                continue
            target = REFS.get((name, c))
            if target:
                vals = v if isinstance(v, list) else [v]
                for x in vals:
                    if x in SPECIAL:
                        continue
                    if target not in ids:
                        broken.append(f"{name}.{r['id']}.{c} -> sheet '{target}' does not exist")
                    elif x not in ids[target]:
                        broken.append(f"{name}.{r['id']}.{c} -> {target}.{x} not found")

def show(title, items, limit=400):
    print(f"\n== {title}: {len(items)}")
    for x in items[:limit]:
        print("  " + x)

show("Unresolved references", broken)
show("Unfilled cells", unfilled)
show("Rows not verified in game", unverified, 0)
total = sum(len(s["rows"]) * len(s["columns"]) for s in sheets.values())
print(f"\n{len(sheets)} sheets, {sum(len(s['rows']) for s in sheets.values())} rows, {total} cells")
strict = "--strict" in sys.argv
ok = not broken and not unfilled and (not strict or not unverified)
print("PREFLIGHT", "CLEAN" if ok else "NOT CLEAN")
sys.exit(0 if ok else 1)
