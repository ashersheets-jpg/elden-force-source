"""Set one cell in a sheet: python sheets/set_cell.py <sheet> <row id> <column> <json value>"""
import json, os, sys

sheet, row, col, raw = sys.argv[1:5]
path = os.path.join(os.path.dirname(os.path.abspath(__file__)), sheet + ".json")
s = json.load(open(path, encoding="utf-8"))
if col not in s["columns"]:
    sys.exit(f"{sheet} has no column {col}")
for r in s["rows"]:
    if r["id"] == row:
        try:
            r[col] = json.loads(raw)
        except json.JSONDecodeError:
            r[col] = raw  # plain text (shells strip the quotes)
        break
else:
    sys.exit(f"{sheet} has no row {row}")
json.dump(s, open(path, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
print(f"{sheet}.{row}.{col} = {raw}")
