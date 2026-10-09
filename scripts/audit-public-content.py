#!/usr/bin/env python3
"""Check publishable files, including compressed DOCX parts and WASM strings.

Run from any directory. Deleted tracked files are skipped. Ignored local files
are not publishable and are excluded; untracked, nonignored files are included.
This supplements a credential scanner such as Gitleaks; it does not replace it.
"""

import io
from pathlib import Path
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PATTERNS = {
    "personal home path": rb"/(?:Users|home)/[A-Za-z0-9_.-]+/|[A-Za-z]:\\+Users\\+[A-Za-z0-9_.-]+",
    "source-system session identifier": rb"sessionId\x3d[^&\s<>\"']+",
    "internal source-system address": rb"(?:dav(?:-blue)?|uspto-oacsxp-fs)\.uspto\.gov",
    "private key": rb"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----",
}


def inspect(label, data):
    findings = []
    for reason, pattern in PATTERNS.items():
        if re.search(pattern, data, re.I):
            findings.append(f"{label}: {reason}")
    if label.endswith(("docProps/core.xml", "docProps/app.xml")):
        try:
            root = ET.fromstring(data)
        except ET.ParseError:
            findings.append(f"{label}: malformed document metadata")
        else:
            for node in root.iter():
                if node.tag.rsplit("}", 1)[-1] in {
                    "creator", "lastModifiedBy", "description", "Company"
                } and node.text and node.text.strip():
                    findings.append(f"{label}: identifying document metadata")
    if label.endswith(("docProps/custom.xml", "customXml/item1.xml")):
        try:
            root = ET.fromstring(data)
        except ET.ParseError:
            findings.append(f"{label}: malformed source-system metadata")
        else:
            if any(node.text and node.text.strip() for node in root.iter()):
                findings.append(f"{label}: source-system metadata requires review")
    return findings


def main():
    paths = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
    ).decode().split("\0")
    findings = []
    count = 0
    for name in sorted(set(paths) - {""}):
        path = ROOT / name
        if not path.is_file():
            continue
        count += 1
        if name.endswith((".wasm", ".whl", ".so", ".dylib", ".dll", ".exe", ".rlib", ".rmeta")) or "/dist/" in name or re.search(r"^packages/[^/]+/wasm/.*\.(?:js|ts)$", name):
            findings.append(f"{name}: compiled output must be ignored")
        if name.startswith(".tmp-comments/"):
            findings.append(f"{name}: temporary Word experiment")
        data = path.read_bytes()
        findings.extend(inspect(name, data))
        if zipfile.is_zipfile(io.BytesIO(data)):
            with zipfile.ZipFile(io.BytesIO(data)) as archive:
                for member in archive.namelist():
                    if not member.endswith("/"):
                        findings.extend(inspect(f"{name}!{member}", archive.read(member)))
    if findings:
        # Print locations and categories only, never credential or metadata values.
        print("\n".join(findings), file=sys.stderr)
        return 1
    print(f"Public-content audit passed ({count} files, including archive contents).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
