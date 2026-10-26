#!/usr/bin/env python3
"""Pack a Beacon plugin into a deterministic archive and write SHA256SUMS."""

import argparse
import gzip
import hashlib
import io
import re
import sys
import tarfile
import tomllib
from pathlib import Path, PurePosixPath


def df_major(plugin_dir: Path, manifest: dict) -> str:
    """Read the DataFusion major version from the Cargo.lock of the native crate."""
    lock = plugin_dir / manifest["native"]["crate"] / "Cargo.lock"
    packages = tomllib.loads(lock.read_text(encoding="utf-8")).get("package", [])
    versions = [p["version"] for p in packages if p["name"] == "datafusion"]
    if len(versions) != 1:
        sys.exit(f"{lock}: expected one datafusion package, found {len(versions)}")
    return versions[0].split(".")[0]


def check_relative(value: str) -> None:
    """Reject manifest paths that are absolute or leave the plugin folder."""
    path = PurePosixPath(value.replace("\\", "/"))
    if path.is_absolute() or ".." in path.parts or re.match(r"[A-Za-z]:", value):
        sys.exit(f"unsafe path in manifest: {value}")


def collect(files: dict) -> list:
    """Return sorted (archive path, is_dir, source) entries for the staged files."""
    entries = {}
    for arc, src in files.items():
        parts = arc.split("/")
        for i in range(1, len(parts)):
            entries["/".join(parts[:i])] = (True, None)
        entries[arc] = (False, src)
    return [(p, d, s) for p, (d, s) in sorted(entries.items())]


def pack(files: dict) -> bytes:
    """Build a gzip tar with fixed mtime, owner and mode, like pack_dir in Rust."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.GNU_FORMAT) as tar:
        for path, is_dir, src in collect(files):
            info = tarfile.TarInfo(path + "/" if is_dir else path)
            info.mtime = 0
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            if is_dir:
                info.type = tarfile.DIRTYPE
                info.mode = 0o755
                tar.addfile(info)
            else:
                data = src.read_bytes()
                info.type = tarfile.REGTYPE
                info.mode = 0o644
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
    out = io.BytesIO()
    with gzip.GzipFile(fileobj=out, mode="wb", mtime=0, filename="") as gz:
        gz.write(buf.getvalue())
    return out.getvalue()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plugin_dir", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--library", type=Path, help="built native library")
    parser.add_argument("--target", help="target triple of the native library")
    args = parser.parse_args()

    plugin_dir = args.plugin_dir
    manifest = tomllib.loads(
        (plugin_dir / "beacon-plugin.toml").read_text(encoding="utf-8")
    )
    plugin = manifest.get("plugin")
    if not isinstance(plugin, dict):
        sys.exit("beacon-plugin.toml needs a [plugin] table with name, version and beacon")
    name, version = plugin["name"], plugin["version"]
    if not re.fullmatch(r"[a-z][a-z0-9_]*", name):
        sys.exit(f"invalid plugin name: {name}")

    files = {"beacon-plugin.toml": plugin_dir / "beacon-plugin.toml"}
    for sql in manifest.get("sql", {}).get("files", []):
        check_relative(sql)
        files[Path(sql).as_posix()] = plugin_dir / sql

    if "native" in manifest:
        check_relative(manifest["native"]["library"])
        if not args.library or not args.target:
            sys.exit("a native plugin needs --library and --target")
        files["lib/" + args.library.name] = args.library
        archive = f"{name}-{version}-df{df_major(plugin_dir, manifest)}-{args.target}.tar.gz"
    else:
        archive = f"{name}-{version}-any.tar.gz"

    args.out.mkdir(parents=True, exist_ok=True)
    data = pack(files)
    (args.out / archive).write_bytes(data)
    digest = hashlib.sha256(data).hexdigest()
    # Binary mode, two spaces: the exact sha256sum format.
    sums = args.out / "SHA256SUMS"
    lines = {}
    if sums.exists():
        for line in sums.read_text(encoding="utf-8").splitlines():
            if line.strip():
                lines[line.split(None, 1)[1].lstrip("*")] = line.split()[0]
    lines[archive] = digest
    text = "".join(f"{h}  {n}\n" for n, h in sorted(lines.items()))
    sums.write_bytes(text.encode())
    print(f"{digest}  {archive}")


if __name__ == "__main__":
    main()
