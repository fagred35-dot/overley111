#!/usr/bin/env python3
"""Нормализатор «сырого» ответа ИИ → чистый JSON прицела (эталон шага M5-4).

Зачем: модели отдают прицел в markdown-заборчиках, с пояснениями до/после, с rgba() вместо
hex, с trailing commas и комментариями. Приложение (M5) и этот скрипт делают одно и то же:
вытаскивают JSON, чинят типовые ошибки, честно перечисляют, что исправили.

Использование:
    python tools/normalize_ai_answer.py answer.md -o my.json
    cat answer.md | python tools/normalize_ai_answer.py - -o my.json
    python tools/normalize_ai_answer.py answer.md --print      #clean-JSON в stdout

Дальше обычная цепочка проверки:
    python tools/validate_presets.py my.json
    python tools/render_preview.py my.json -o my.png

Это референс на Python; продакшен-версия на Rust появится в M5-4 и обязана проходить
тот же корпус ответов (schema/tests/ai-roundtrip/).
"""
from __future__ import annotations

import argparse
import json
import re
import sys

NAMED_COLORS = {
    "red": "#FF0000", "green": "#008000", "lime": "#00FF00", "blue": "#0000FF",
    "white": "#FFFFFF", "black": "#000000", "yellow": "#FFFF00", "cyan": "#00FFFF",
    "aqua": "#00FFFF", "magenta": "#FF00FF", "fuchsia": "#FF00FF", "orange": "#FFA500",
    "purple": "#800080", "violet": "#EE82EE", "pink": "#FFC0CB", "gray": "#808080",
    "grey": "#808080", "silver": "#C0C0C0", "gold": "#FFD700", "teal": "#008080",
    "navy": "#000080", "olive": "#808000", "maroon": "#800000",
}


def strip_fences(text: str, fixes: list[str]) -> str:
    if "```" in text:
        text = re.sub(r"```[a-zA-Z]*\n?", "", text)
        fixes.append("сняты markdown-заборчики ```")
    return text


def extract_json_object(text: str) -> str:
    """Первый сбалансированный {...} с учётом строк и экранирования."""
    start = text.find("{")
    if start < 0:
        raise ValueError("в тексте нет ни одного '{' — это не JSON-ответ")
    depth = 0
    in_str = False
    esc = False
    for i in range(start, len(text)):
        ch = text[i]
        if in_str:
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == '"':
                in_str = False
            continue
        if ch == '"':
            in_str = True
        elif ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return text[start:i + 1]
    raise ValueError("не нашёл закрывающую '}' — JSON обрезан")


def remove_trailing_commas(src: str, fixes: list[str]) -> str:
    out = []
    in_str = False
    esc = False
    pending_comma = False
    for ch in src:
        if in_str:
            out.append(ch)
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == '"':
                in_str = False
            continue
        if ch == '"':
            in_str = True
            if pending_comma:
                out.append(",")
                pending_comma = False
            out.append(ch)
            continue
        if pending_comma:
            if ch.isspace():
                out.append(ch)
                continue
            if ch in "}]":
                fixes.append("убраны висячие запятые перед }/]")
                pending_comma = False
                out.append(ch)
                continue
            out.append(",")
            pending_comma = False
        if ch == ",":
            pending_comma = True
            continue
        out.append(ch)
    if pending_comma:
        out.append(",")
    return "".join(out)


def remove_line_comments(src: str, fixes: list[str]) -> str:
    out = []
    in_str = False
    esc = False
    i = 0
    changed = False
    while i < len(src):
        ch = src[i]
        if in_str:
            out.append(ch)
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == '"':
                in_str = False
            i += 1
            continue
        if ch == '"':
            in_str = True
            out.append(ch)
            i += 1
            continue
        if ch == "/" and i + 1 < len(src) and src[i + 1] == "/":
            while i < len(src) and src[i] != "\n":
                i += 1
            changed = True
            continue
        out.append(ch)
        i += 1
    if changed:
        fixes.append("удалены //-комментарии")
    return "".join(out)


def fix_colors(src: str, fixes: list[str]) -> str:
    def rgba(m: re.Match) -> str:
        parts = [p.strip() for p in m.group(1).split(",")]
        vals = []
        for p in parts[:3]:
            v = float(p)
            vals.append(int(round(v if v > 1 else v * 255)))
        a = 255
        if len(parts) == 4:
            av = float(parts[3])
            a = int(round(av if av > 1 else av * 255))
        fixes.append("rgba()/rgb() переведены в hex")
        return f'"#{vals[0]:02X}{vals[1]:02X}{vals[2]:02X}{a:02X}"'

    src = re.sub(r'"rgba?\(([^)]*)\)"', rgba, src)

    def named(m: re.Match) -> str:
        name = m.group(1).lower()
        if name in NAMED_COLORS:
            fixes.append(f"именованной цвет '{name}' переведён в hex")
            return f'"{NAMED_COLORS[name]}FF"'
        return m.group(0)

    return re.sub(r'"([A-Za-z]+)"(?=\s*[,}])', named, src) if False else re.sub(
        r'"\s*(red|green|lime|blue|white|black|yellow|cyan|aqua|magenta|fuchsia|orange|purple|violet|pink|gray|grey|silver|gold|teal|navy|olive|maroon)\s*"',
        named, src)


def normalize(raw: str) -> tuple[str, list[str]]:
    fixes: list[str] = []
    text = strip_fences(raw, fixes)
    body = extract_json_object(text)
    body = remove_line_comments(body, fixes)
    body = remove_trailing_commas(body, fixes)
    body = fix_colors(body, fixes)
    json.loads(body)  # бросит ValueError с понятной позицией, если чинить нечего и JSON битый
    return body, sorted(set(fixes))


def main(argv):
    ap = argparse.ArgumentParser(description="Нормализатор ответа ИИ → чистый .ovcross JSON")
    ap.add_argument("src", help="файл с ответом ИИ или '-' для stdin")
    ap.add_argument("-o", "--out", help="куда сохранить чистый JSON")
    ap.add_argument("--print", dest="to_stdout", action="store_true", help="печатать чистый JSON в stdout")
    args = ap.parse_args(argv)

    raw = sys.stdin.read() if args.src == "-" else open(args.src, encoding="utf-8").read()
    try:
        clean, fixes = normalize(raw)
    except (ValueError, json.JSONDecodeError) as exc:
        print(f"НЕ РАЗОБРАНО: {exc}", file=sys.stderr)
        print("Совет: нажми в приложении «Копировать запрос на исправление» и отправь его модели.", file=sys.stderr)
        return 1

    for f in fixes:
        print(f"  • исправлено автоматически: {f}", file=sys.stderr)
    doc = json.loads(clean)
    pretty = json.dumps(doc, ensure_ascii=False, indent=2)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as fh:
            fh.write(pretty + "\n")
        print(f"  → сохранено: {args.out}", file=sys.stderr)
    if args.to_stdout or not args.out:
        print(pretty)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
