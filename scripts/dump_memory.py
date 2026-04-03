#!/usr/bin/env python3
"""Dump mobileclaw memory.db documents to stdout.

Usage:
  dump_memory.py [DB_PATH] [options]

Examples:
  dump_memory.py                                  # default: ~/.mobileclaw/memory.db, all rows
  dump_memory.py build/memory.db                  # custom db path
  dump_memory.py build/memory.db -n 20            # last 20 rows
  dump_memory.py build/memory.db -c conversation  # filter by category
  dump_memory.py build/memory.db -p history/      # filter by path prefix
  dump_memory.py build/memory.db -n 10 --json     # JSON output
"""

import argparse
import json
import os
import sqlite3
import sys
from datetime import datetime, timezone


def ts(epoch: int) -> str:
    return datetime.fromtimestamp(epoch, tz=timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")


def default_db() -> str:
    return os.path.join(os.environ.get("HOME", "."), ".mobileclaw", "memory.db")


def main():
    p = argparse.ArgumentParser(description="Dump mobileclaw memory.db")
    p.add_argument("db", nargs="?", default=default_db(), help="Path to memory.db")
    p.add_argument("-n", "--limit", type=int, default=0, help="Max rows (0 = all)")
    p.add_argument("-c", "--category", default="", help="Filter by category (e.g. conversation)")
    p.add_argument("-p", "--prefix", default="", help="Filter by path prefix (e.g. history/)")
    p.add_argument("--json", action="store_true", help="Output as JSON array")
    args = p.parse_args()

    if not os.path.exists(args.db):
        print(f"error: db not found: {args.db}", file=sys.stderr)
        sys.exit(1)

    conn = sqlite3.connect(args.db)
    conn.row_factory = sqlite3.Row

    sql = "SELECT id, path, category, content, created_at, updated_at FROM documents WHERE 1=1"
    params = []

    if args.category:
        sql += " AND category = ?"
        params.append(args.category)
    if args.prefix:
        escaped = args.prefix.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
        sql += " AND path LIKE ? ESCAPE '\\'"
        params.append(escaped + "%")

    sql += " ORDER BY created_at ASC"

    if args.limit > 0:
        # Show the last N rows when a limit is given
        sql = f"SELECT * FROM ({sql}) ORDER BY created_at DESC LIMIT ?"
        params.append(args.limit)
        sql = f"SELECT * FROM ({sql}) ORDER BY created_at ASC"

    rows = conn.execute(sql, params).fetchall()

    total = conn.execute(
        "SELECT COUNT(*) FROM documents" +
        (" WHERE category = ?" if args.category else "") +
        (" AND path LIKE ? ESCAPE '\\'" if args.prefix else ""),
        [a for a in [args.category or None, ((args.prefix.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%") if args.prefix else None)] if a is not None],
    ).fetchone()[0]

    conn.close()

    if args.json:
        out = [
            {
                "id": r["id"],
                "path": r["path"],
                "category": r["category"],
                "content": r["content"],
                "created_at": r["created_at"],
                "updated_at": r["updated_at"],
            }
            for r in rows
        ]
        print(json.dumps(out, ensure_ascii=False, indent=2))
        return

    shown = len(rows)
    print(f"memory.db: {args.db}")
    print(f"showing {shown}/{total} rows" +
          (f" (category={args.category})" if args.category else "") +
          (f" (prefix={args.prefix})" if args.prefix else ""))
    print("─" * 80)

    for r in rows:
        content_preview = r["content"].replace("\n", " ↵ ")
        if len(content_preview) > 200:
            content_preview = content_preview[:200] + "…"
        print(f"path     : {r['path']}")
        print(f"category : {r['category']}")
        print(f"created  : {ts(r['created_at'])}")
        print(f"content  : {content_preview}")
        print("─" * 80)


if __name__ == "__main__":
    main()
