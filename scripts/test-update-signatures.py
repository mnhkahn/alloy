#!/usr/bin/env python3
"""Check the real Sparkle archive/feed and reject tampered copies (local Keychain)."""
import argparse
import pathlib
import subprocess
import tempfile
import xml.etree.ElementTree as ET

parser = argparse.ArgumentParser()
parser.add_argument("appcast", type=pathlib.Path)
parser.add_argument("--sdk", type=pathlib.Path, default=pathlib.Path("target/sparkle-sdk"))
parser.add_argument("--account", default="com.mnhkahn.alloy")
parser.add_argument("--ed-key-file", type=pathlib.Path)
args = parser.parse_args()
verify = [str(args.sdk / "bin/sign_update"), "--account", args.account, "--verify"]
if args.ed_key_file: verify += ["--ed-key-file", str(args.ed_key_file)]
ns = "{http://www.andymatuschak.org/xml-namespaces/sparkle}"
feed = ET.parse(args.appcast)
enclosure = feed.find("./channel/item/enclosure")
assert enclosure is not None, "Missing update item"
signature = enclosure.attrib[ns + "edSignature"]
archive = args.appcast.parent / pathlib.PurePosixPath(enclosure.attrib["url"]).name
assert archive.stat().st_size == int(enclosure.attrib["length"])
subprocess.run(verify + [str(args.appcast)], check=True)
subprocess.run(verify + [str(archive), signature], check=True)
with tempfile.TemporaryDirectory() as folder:
    bad_archive = pathlib.Path(folder) / "tampered.zip"
    bad_archive.write_bytes(archive.read_bytes() + b"tampered")
    result = subprocess.run(verify + [str(bad_archive), signature], capture_output=True)
    assert result.returncode != 0, "Tampered archive was accepted"
    bad_feed = pathlib.Path(folder) / "tampered.xml"
    bad_feed.write_bytes(args.appcast.read_bytes().replace(b"<channel>", b"<channel>\n<!-- tampered -->", 1))
    result = subprocess.run(verify + [str(bad_feed)], capture_output=True)
    assert result.returncode != 0, "Tampered feed was accepted"
print("Valid archive/feed accepted; tampered archive/feed rejected")
