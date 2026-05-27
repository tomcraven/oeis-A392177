#!/usr/bin/env python3
"""
Two-knight placement reference from src/a392177_2.py (parameterized).

Total ``turns`` equals K * N in the original script (K=2 armies, N placements each).
Outputs spiral indices in turn order (army 0, army 1, army 0, …) as JSON when --json.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from typing import Iterable

K = 2
KNIGHT = (2, 1)


def step(t: tuple[int, int]) -> tuple[int, int]:
    (a, b) = t
    if (a, b) == (0, 0):
        return (1, 0)
    if a > abs(b):
        return (a, b + 1)
    if a < -abs(b):
        return (a, b - 1)
    if b > abs(a):
        return (a - 1, b)
    if b < -abs(a):
        return (a + 1, b)
    if a == b and a > 0:
        return (a - 1, b)
    if a == b and a < 0:
        return (a + 1, b)
    if a == -b and a > 0:
        return (a + 1, b)
    if a == -b and a < 0:
        return (a, b - 1)
    raise RuntimeError(f"step stuck at {t}")


def reachable(t: tuple[int, int]) -> list[tuple[int, int]]:
    x, y = t
    m, n = KNIGHT
    return [
        (x + m, y + n),
        (x + m, y - n),
        (x - m, y + n),
        (x - m, y - n),
        (x + n, y + m),
        (x - n, y + m),
        (x + n, y - m),
        (x - n, y - m),
    ]


def shell(p: tuple[int, int]) -> int:
    return max(abs(p[0]), abs(p[1]))


def run_sim(total_placements: int, k: int = K) -> list[list[tuple[int, int]]]:
    candidates = [(0, 0) for _ in range(k)]
    histories: list[list[tuple[int, int]]] = [[] for _ in range(k)]
    verboten: list[set[tuple[int, int]]] = [set() for _ in range(k)]

    for i in range(total_placements):
        index = i % k
        candidate = candidates[index]
        while candidate in verboten[index]:
            candidate = step(candidate)
        candidates[index] = candidate
        histories[index].append(candidate)
        threatened = reachable(candidate)
        for j in range(k):
            if j == index:
                verboten[j].update([candidate])
            else:
                verboten[j].update([candidate, *threatened])
    return histories


def build_spiral_index_map(histories: list[list[tuple[int, int]]]) -> dict[tuple[int, int], int]:
    b_max = histories[0][-1]
    r_max = histories[1][-1]
    s = max(shell(b_max), shell(r_max)) + 1
    dic: dict[tuple[int, int], int] = {}
    p = (0, 0)
    i = 0
    for _ in range(4 * s * (s + 3)):
        dic[p] = i
        p = step(p)
        i += 1
    return dic


def histories_to_spiral_indices(
    histories: list[list[tuple[int, int]]],
) -> list[list[int]]:
    dic = build_spiral_index_map(histories)
    return [[dic[p] for p in h] for h in histories]


def placement_checksum_interleaved(indices_by_army: list[list[int]]) -> int:
    """Matches Rust ``sim.placements`` order and ``time_sim`` / ``sim`` tests."""
    k = len(indices_by_army)
    n = len(indices_by_army[0])
    h = 0xCBF29CE484222325
    for t in range(n):
        for army_id in range(k):
            index = indices_by_army[army_id][t]
            value = (index << 8) ^ army_id
            h ^= value
            h = (h * 0x00000100000001B3) & ((1 << 64) - 1)
    return h


def interleaved_placements(indices_by_army: list[list[int]]) -> list[tuple[int, int]]:
    k = len(indices_by_army)
    n = len(indices_by_army[0])
    out: list[tuple[int, int]] = []
    for t in range(n):
        for army_id in range(k):
            out.append((indices_by_army[army_id][t], army_id))
    return out


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "turns",
        type=int,
        nargs="?",
        default=10_000,
        help="Total placements (K*N in a392177_2.py)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print one JSON object (checksum, timing, last indices)",
    )
    parser.add_argument(
        "--write-indices",
        metavar="PATH",
        help="Write interleaved spiral indices one per line (rust diff friendly)",
    )
    args = parser.parse_args(list(argv) if argv is not None else None)

    if args.turns < 1:
        print("turns must be >= 1", file=sys.stderr)
        return 2

    t0 = time.perf_counter()
    histories = run_sim(args.turns)
    indices = histories_to_spiral_indices(histories)
    elapsed_s = time.perf_counter() - t0
    checksum = placement_checksum_interleaved(indices)

    if args.write_indices:
        with open(args.write_indices, "w", encoding="utf-8") as f:
            for index, _army in interleaved_placements(indices):
                f.write(f"{index}\n")

    payload = {
        "engine": "python_reference",
        "preset": "knight_2_pairwise",
        "turns": args.turns,
        "placements": args.turns,
        "checksum": checksum,
        "elapsed_s": elapsed_s,
        "black_last_index": indices[0][-1],
        "red_last_index": indices[1][-1],
    }

    if args.json:
        print(json.dumps(payload))
    else:
        print(
            f"turns={args.turns} checksum={checksum} "
            f"elapsed_s={elapsed_s:.6f} "
            f"black_last={indices[0][-1]} red_last={indices[1][-1]}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
