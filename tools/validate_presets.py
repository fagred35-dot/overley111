#!/usr/bin/env python3
"""Валидатор пресетов Overley против Crosshair DSL v1.

Использование:
    python tools/validate_presets.py                 # проверить presets/ + schema/tests/
    python tools/validate_presets.py path/to/a.json  # проверить конкретные файлы

Код возврата: 0 — всё хорошо, 1 — есть ошибки валидации, 2 — ошибка окружения.
Зависимости: pip install jsonschema (>= 4.18, draft 2020-12 + unevaluatedProperties).
"""
from __future__ import annotations

import glob
import json
import os
import sys

try:
    from jsonschema import Draft202012Validator
except ImportError:  # pragma: no cover
    print("Нужен пакет jsonschema: pip install jsonschema", file=sys.stderr)
    raise SystemExit(2)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCHEMA_PATH = os.path.join(ROOT, "schema", "crosshair-v1.schema.json")

# Дополнительные проверки, которые JSON Schema не выражает (или выражает дорого).
MAX_FILE_BYTES = 512 * 1024
MAX_TOTAL_LAYERS = 64
MAX_GROUP_DEPTH = 4
FORBIDDEN_TOKENS = (
    "http://", "https://", "//", "<script", "javascript:", "eval(", "function(",
    "require(", "import(", "fetch(", "document.", "window.", "__proto__",
)


def load_schema() -> Draft202012Validator:
    with open(SCHEMA_PATH, encoding="utf-8") as fh:
        schema = json.load(fh)
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def count_layers(layers, depth=0):
    total, max_depth = 0, depth
    for layer in layers:
        total += 1
        if layer.get("type") == "group":
            nested, nested_depth = count_layers(layer.get("layers", []), depth + 1)
            total += nested
            max_depth = max(max_depth, nested_depth)
    return total, max_depth


def semantic_check(doc) -> list[str]:
    """Проверки поверх схемы: лимиты, запрещённые токены, глубина вложенности."""
    problems: list[str] = []
    raw = json.dumps(doc, ensure_ascii=False).lower()
    for token in FORBIDDEN_TOKENS:
        if token in raw:
            problems.append(f"запрещённый токен в документе: {token!r}")

    layers = doc.get("layers", [])
    total, depth = count_layers(layers)
    if total > MAX_TOTAL_LAYERS:
        problems.append(f"слишком много слоёв: {total} > {MAX_TOTAL_LAYERS}")
    if depth > MAX_GROUP_DEPTH:
        problems.append(f"слишком глубокая вложенность group: {depth} > {MAX_GROUP_DEPTH}")

    for binding in doc.get("reactive", {}).get("bindings", []):
        if not binding.get("range"):
            problems.append(f"reactive binding без range (нужен guard): {binding.get('target')}")
    return problems


def validate_file(path: str, validator: Draft202012Validator) -> list[str]:
    errors: list[str] = []
    size = os.path.getsize(path)
    if size > MAX_FILE_BYTES:
        errors.append(f"файл больше лимита: {size} > {MAX_FILE_BYTES} байт")
        return errors
    try:
        with open(path, encoding="utf-8") as fh:
            doc = json.load(fh)
    except (OSError, json.JSONDecodeError) as exc:
        return [f"не читается как JSON: {exc}"]

    for err in sorted(validator.iter_errors(doc), key=lambda e: list(e.path)):
        where = "/".join(str(p) for p in err.absolute_path) or "<root>"
        errors.append(f"{where}: {err.message[:200]}")
    errors.extend(semantic_check(doc))
    return errors


def main(argv: list[str]) -> int:
    validator = load_schema()

    if len(argv) > 1:
        targets = argv[1:]
    else:
        targets = sorted(
            glob.glob(os.path.join(ROOT, "presets", "**", "*.json"), recursive=True)
        )
        # Негативные тесты: файлы в schema/tests/invalid ОБЯЗАНЫ не проходить.
        invalid_dir = os.path.join(ROOT, "schema", "tests", "invalid")
        expected_fail = sorted(glob.glob(os.path.join(invalid_dir, "*.json")))
        bad = 0
        for path in expected_fail:
            errs = validate_file(path, validator)
            if errs:
                print(f"OK  (отклонён, как и должен) {os.path.relpath(path, ROOT)}: {errs[0][:90]}")
            else:
                bad += 1
                print(f"FAIL (прошёл, но должен быть отклонён) {os.path.relpath(path, ROOT)}")
        if bad:
            return 1

    failed = 0
    for path in targets:
        errors = validate_file(path, validator)
        rel = os.path.relpath(path, ROOT)
        if errors:
            failed += 1
            print(f"FAIL {rel}")
            for e in errors[:10]:
                print(f"     - {e}")
        else:
            print(f"OK   {rel}")
    print(f"\nИтого: {len(targets) - failed}/{len(targets)} файлов валидны")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
