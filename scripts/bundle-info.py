#!/usr/bin/env python3
"""Validate release versions and generate the macOS bundle metadata."""
import argparse
import pathlib
import plistlib
import re


def version_number(tag):
    if not re.fullmatch(r"[vV]?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
        raise ValueError("Expected a stable version such as v1.2.3, V1.2.3 or 1.2.3")
    return tag.lstrip("vV")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", default="")
    parser.add_argument("--output", type=pathlib.Path)
    args = parser.parse_args()
    tag = args.version
    if not tag:
        manifest = (pathlib.Path(__file__).resolve().parent.parent / "Cargo.toml").read_text()
        tag = re.search(r'^version\s*=\s*"([^"]+)"', manifest, re.MULTILINE).group(1)
    try:
        version = version_number(tag)
    except ValueError as error:
        parser.error(str(error))
    if args.output:
        info = {
            "CFBundleName": "Alloy", "CFBundleDisplayName": "Alloy",
            "CFBundleIdentifier": "com.mnhkahn.alloy", "CFBundleExecutable": "Alloy",
            "CFBundleIconFile": "AppIcon", "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version, "CFBundleVersion": version,
            "LSMinimumSystemVersion": "12.0", "NSHighResolutionCapable": True,
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(plistlib.dumps(info))
    print(version)


if __name__ == "__main__":
    main()
