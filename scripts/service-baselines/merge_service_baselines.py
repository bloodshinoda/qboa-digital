#!/usr/bin/env python3
"""Merge measured QBoa Windows service baseline JSON files into matrix and long CSVs."""
from __future__ import annotations
import argparse, csv, json, re, sys
from pathlib import Path

LABELS = {0: "Boot", 1: "System", 2: "Automatic", 3: "Manual", 4: "Disabled"}

def normalized_label(service: dict) -> str:
    start = service.get("start")
    if start == 2 and int(service.get("delayed_autostart") or 0) == 1:
        return "Automatic (Delayed)"
    return service.get("start_label") or LABELS.get(start, "Unknown")

def read_capture(path: Path) -> dict:
    data = json.loads(path.read_text(encoding="utf-8-sig"))
    if data.get("source") != "measured":
        raise ValueError(f"{path}: source deve ser 'measured'; não aceite inferência como medição")
    env = data.get("environment")
    if not isinstance(env, str) or not env.strip():
        raise ValueError(f"{path}: campo environment ausente")
    os_info = data.get("os") or {}
    services = data.get("services")
    if not isinstance(services, list):
        raise ValueError(f"{path}: services deve ser uma lista")
    by_name = {}
    for service in services:
        name = service.get("name")
        if not isinstance(name, str) or not name:
            raise ValueError(f"{path}: serviço sem nome")
        if re.search(r"_[0-9a-fA-F]{4,}$", name):
            raise ValueError(f"{path}: instância dinâmica por sessão não deve estar no baseline: {name}")
        if name in by_name:
            raise ValueError(f"{path}: nome duplicado: {name}")
        by_name[name] = service
    return {"path": path.name, "environment": env, "os": os_info, "services": by_name}

def merge(paths: list[Path], outdir: Path) -> tuple[int, int]:
    captures = [read_capture(p) for p in paths]
    envs = [c["environment"] for c in captures]
    if len(set(envs)) != len(envs):
        raise ValueError("environment duplicado; cada captura precisa de um nome único")
    outdir.mkdir(parents=True, exist_ok=True)
    names = sorted({name for c in captures for name in c["services"]}, key=str.casefold)
    matrix_path = outdir / "service-matrix.csv"
    observation_path = outdir / "service-observations.csv"
    env_meta = {c["environment"]: c["os"] for c in captures}
    with matrix_path.open("w", newline="", encoding="utf-8-sig") as f:
        fields = ["service", *envs, "varies_between_envs", "source"]
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for name in names:
            values, sources, present = {}, {}, []
            for c in captures:
                svc = c["services"].get(name)
                if svc is None:
                    values[c["environment"]] = ""
                    continue
                label = normalized_label(svc)
                if svc.get("trigger_present"):
                    label += " (Trigger)"
                values[c["environment"]] = label
                sources[c["environment"]] = svc.get("source", "measured")
                present.append(label)
            writer.writerow({"service": name, **values,
                "varies_between_envs": str(len(set(present)) > 1).lower(),
                "source": json.dumps(sources, ensure_ascii=False, sort_keys=True)})
    with observation_path.open("w", newline="", encoding="utf-8-sig") as f:
        fields = ["environment", "product_name", "display_version", "build", "ubr", "edition_id",
            "service", "start", "delayed_autostart", "start_label", "trigger_present",
            "type", "depend_on_service", "source", "capture_file"]
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for c in captures:
            meta = c["os"]
            for name, svc in sorted(c["services"].items(), key=lambda item: item[0].casefold()):
                writer.writerow({
                    "environment": c["environment"], "product_name": meta.get("product_name", ""),
                    "display_version": meta.get("display_version", ""), "build": meta.get("build", ""),
                    "ubr": meta.get("ubr", ""), "edition_id": meta.get("edition_id", ""),
                    "service": name, "start": svc.get("start", ""),
                    "delayed_autostart": svc.get("delayed_autostart", ""),
                    "start_label": normalized_label(svc),
                    "trigger_present": str(bool(svc.get("trigger_present"))).lower(),
                    "type": svc.get("type", ""),
                    "depend_on_service": json.dumps(svc.get("depend_on_service", []), ensure_ascii=False),
                    "source": svc.get("source", "measured"), "capture_file": c["path"]})
    (outdir / "capture-metadata.json").write_text(json.dumps({
        "schema_version": 1, "environments": env_meta,
        "captures": [{"environment": c["environment"], "file": c["path"], "source": "measured"} for c in captures]
    }, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return len(names), sum(len(c["services"]) for c in captures)

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True, help="Diretório de saída")
    parser.add_argument("captures", nargs="+", type=Path, help="JSONs produzidos pelo coletor")
    args = parser.parse_args()
    try:
        names, observations = merge(args.captures, args.out)
    except (OSError, json.JSONDecodeError, ValueError) as exc:
        print(f"Erro: {exc}", file=sys.stderr)
        return 2
    print(f"Matriz criada: {names} serviços únicos; {observations} observações medidas; saída {args.out}")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
