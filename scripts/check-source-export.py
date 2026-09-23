#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Inspect the explicit source-release allowlist; no runtime data is included."""
import base64
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
DIRECTORIES = ("compat", "scripts", "docs", "tests", "flightdeck", "ui", ".github")
TOP_LEVEL = ("README.md", "BUILDING.md", "LICENSE", "pyproject.toml", ".gitignore",
             "install.sh", "Install Flightdeck.desktop", "build_support.py", "MANIFEST.in")
SKIP = {"__pycache__", "node_modules", "build", "dist", "artifacts", "coverage", ".git", ".venv", ".pytest_cache"}
EXTENSIONS = {".py", ".sh", ".md", ".txt", ".json", ".toml", ".lock", ".patch",
              ".c", ".h", ".cpp", ".hpp", ".inc", ".idl", ".spec", ".def",
              ".css", ".js", ".mjs", ".html", ".svg", ".example", ".yml", ".yaml"}
NAMES = {"LICENSE", "COPYING", "COPYING.LIB", ".gitignore", "Makefile", "xodus-wine-launch"}
# Permit only individually reviewed binary assets, never arbitrary local files.
ASSETS = {
    "docs/images/launcher-overview.png": "1c59eecd1cebab38c03665026b363abbcf4c16189daf5ff4d69f971560fff39b",
    "ui/flight-panorama.png": "f64fc375e0aaa806c4de91cd91ec8f18994a06ff64aa80d8b6d95f7c463c9304",
    "ui/flight-panorama-2020.png": "cd81013284d468fe6d306438e66d9a0d7ee35d6bda02ec2b44dea010f8cf8344",
    "ui/manrope-variable.woff2": "30b83738add8c9edd9e3450b98036a9a8fb5668d0cbd4eb0ce5fe6761197f21f",
}
TEMPLATES = {".example", ".sample", ".template"}
PRIVATE_DIRECTORIES = {".ssh", ".aws", ".azure", "private", "local storage", "indexeddb"}
PRIVATE_NAMES = {".netrc", "_netrc", ".git-credentials", "cookies", "login data",
                 "credentials", "accounts", "token_cache", "msal_token_cache", "xodus-keyring"}
PRIVATE_STEMS = PRIVATE_NAMES | {"account", "token", "tokens", "cookies", "storage-state", "storage_state"}
PRIVATE_FORMATS = {".json", ".ron", ".txt", ".yaml", ".yml", ".db", ".sqlite", ".sqlite3"}
JWT = re.compile(r"(?<![\w-])([A-Za-z0-9_-]{10,4096})\.([A-Za-z0-9_-]{4,32768})\.([A-Za-z0-9_-]{40,2048})(?![\w-])")
KEY_BLOCK = re.compile(r"-----BEGIN ((?:(?:RSA|EC|DSA|OPENSSH|ENCRYPTED) )?PRIVATE KEY|PGP PRIVATE KEY BLOCK)-----"
                       r"(.{32,65536}?)-----END \1-----", re.DOTALL)


def private_path(relative):
    """Check whole names, not source identifiers such as store_account.py."""
    parts = [part.lower() for part in relative.parts]
    if any(part in PRIVATE_DIRECTORIES for part in parts[:-1]):
        return True
    name = parts[-1]
    if Path(name).suffix in TEMPLATES:
        return False  # Examples still receive every content check below.
    return (name == ".env" or name.startswith(".env.") or name in PRIVATE_NAMES or
            (Path(name).stem in PRIVATE_STEMS and Path(name).suffix in PRIVATE_FORMATS))


def decode64(value):
    return base64.b64decode(value + "=" * (-len(value) % 4), altchars=b"-_", validate=True)


def opaque_literal(value):
    # Skip short documented placeholders and formatting expressions; do not
    # exempt test directories or trust labels such as "synthetic" for secrets.
    return (32 <= len(value) <= 65536 and len(set(value)) >= 12 and
            any(c.isdigit() for c in value) and any(c.isalpha() for c in value))


def private_content(text):
    """Conservative recognizable material checks, not a complete secret audit."""
    for block in KEY_BLOCK.finditer(text.replace("\\n", "\n")):
        body = "".join(line.strip().removeprefix("+").removeprefix("-")
                       for line in block[2].splitlines() if ":" not in line)
        if re.fullmatch(r"[A-Za-z0-9+/=\s]+", body) and len(body.strip()) >= 40:
            return "Private key material"
    for match in JWT.finditer(text):
        try:
            header, payload = (json.loads(decode64(part)) for part in match.groups()[:2])
            signature = decode64(match[3])
            if (isinstance(header, dict) and isinstance(payload, dict) and len(signature) >= 32 and
                    isinstance(header.get("alg"), str) and
                    header.get("alg") in {"HS256", "HS384", "HS512", "RS256", "RS384", "RS512",
                                          "ES256", "ES384", "ES512", "PS256", "PS384", "PS512", "EdDSA"}):
                return "Embedded signed-token data"
        except (ValueError, UnicodeError):
            pass
    # Parse each actual JSON object with a key-type field. Bare source-code
    # identifiers or independently documented fields are not private JWKs.
    for match in re.finditer(r'"kty"\s*:', text):
        start = text.rfind("{", 0, match.start())
        if start < 0:
            continue
        try:
            item, _ = json.JSONDecoder().raw_decode(text[start:])
            if not isinstance(item, dict):
                continue
            field = "k" if item.get("kty") == "oct" else "d"
            if (isinstance(item.get("kty"), str) and item["kty"] in {"RSA", "EC", "OKP", "oct"} and
                    isinstance(item.get(field), str)):
                if len(decode64(item[field])) >= 16:
                    return "Private JWK material"
        except (ValueError, UnicodeError):
            pass
    for match in re.finditer(r"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{60,255}|"
                             r"sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{32,255})\b", text):
        if opaque_literal(match[0]):
            return "Embedded access-key data"
    literals = re.compile(r'''(?i)["']?\b(?:access_?token|refresh_?token|client_?secret|api_?key|password|_authToken)["']?\s*[:=]\s*["']([A-Za-z0-9+/=_~.-]{32,65536})["']''')
    headers = re.compile(r"(?i)\b(?:Bearer\s+|XBL3\.0 x=[0-9]{1,20};)([A-Za-z0-9+/=_~.-]{32,65536})")
    if any(opaque_literal(match[1]) for pattern in (literals, headers) for match in pattern.finditer(text)):
        return "Embedded credential literal"
    return None


def source_files():
    files = []
    for name in TOP_LEVEL:
        path = ROOT / name
        if path.is_symlink():
            raise ValueError("Source export refuses symlinks: " + name)
        if path.is_file():
            files.append(path)
    for directory in DIRECTORIES:
        for path in sorted((ROOT / directory).rglob("*")):
            relative = path.relative_to(ROOT)
            if any(part in SKIP for part in relative.parts):
                continue
            if path.is_symlink():
                raise ValueError("Source export refuses symlinks: " + str(relative))
            if not path.is_file():
                continue
            if private_path(relative):
                raise ValueError("Private data file in source export: " + str(relative))
            env_template = path.name.startswith(".env.") and path.suffix in TEMPLATES
            if str(relative) not in ASSETS and path.suffix not in EXTENSIONS and path.name not in NAMES and not env_template:
                raise ValueError("Unrecognized source-release file: " + str(relative))
            files.append(path)
    return sorted(set(files))


def inspect():
    files = source_files()
    hashes = {}
    for path in files:
        relative = str(path.relative_to(ROOT))
        data = path.read_bytes()
        if relative in ASSETS:
            if hashlib.sha256(data).hexdigest() != ASSETS[relative]:
                raise ValueError("Unreviewed artwork in source export: " + relative)
            hashes[relative] = ASSETS[relative]
            continue
        if data.startswith((b"MZ", b"\x7fELF", b"PK\x03\x04", b"\x1f\x8b")) or b"\0" in data:
            raise ValueError("Binary content in source export: " + relative)
        text = data.decode("utf-8")
        # Reject actual machine paths; generic placeholders and code are fine.
        if re.search(r"/(?:home|Users)/[a-zA-Z0-9_.-]+/", text):
            raise ValueError("Personal absolute path in source export: " + relative)
        category = private_content(text)
        if category:
            raise ValueError(category + " in source export: " + relative)
        hashes[relative] = hashlib.sha256(data).hexdigest()
    return {"format": 1, "status": "PASS", "files": hashes}


if __name__ == "__main__":
    result = inspect()
    print(json.dumps({"status": result["status"], "source_files": len(result["files"])}))
