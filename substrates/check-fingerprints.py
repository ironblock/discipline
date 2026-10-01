#!/usr/bin/env python3
"""Every equipment entry's `hardware_fingerprint` covers everything its type declares.

WHY THIS IS A CHECK AND NOT A CONVENTION. The fingerprint is a digest over an
entry's declared hardware fields. The first version of it used one fixed field
list across every entry and silently covered TWO of twelve fields on the two
Apple machines, because their entries name `chip` and `model_identifier` where
the accelerator host names `accelerator` and `board`. It produced a
sixty-four-character digest and looked exactly like a working one.

That is the failure this file exists to make impossible to repeat: a fingerprint
covering fewer fields than its type declares is a FAILURE, not a smaller
fingerprint. The same mistake made silently a second time is indistinguishable
from the first, so it gets a test.

THE RULES, all four enforced rather than remembered:

  1. Every entry names an `entry_type` the registry declares.
  2. Every field that type declares is PRESENT on the entry.
  3. `hardware_fingerprint` is the sha256 of exactly those fields, canonically
     serialised -- so a reader recomputes it from published data.
  4. A declared field may not be an instance field (`os`, or anything naming a
     deployment) and may not be marked inferred. A hardware fingerprint that
     moves on an operating-system update is not one, and an unverified claim
     baked into an identifier is one nobody can correct later without changing
     the identity of every record that cites it.

THE ENGINE RULE (#202). A substrate's `engine_identity` was the sha256 of the
server executable alone. On a llama.cpp build that executable is a stub of about
eighteen kilobytes and the engine is in shared objects beside it (libllama,
libggml-cuda, libllama-server-impl), so two builds with different library code
could share the exe digest and the fingerprint would not move. So every
substrate whose `engine_identity` is a 64-hex digest carries exactly one of:

  * `engine_libraries`, a table of basename -> sha256, with `engine_fingerprint`
    the sha256 of {"exe": engine_identity, "libraries": engine_libraries},
    canonically serialised, and `engine_libraries_read` saying how they were read;
  * `engine_libraries_unreadable`, saying why they can no longer be read;
  * `engine_single_digest_suffices`, saying why one digest is the whole engine
    (a static binary, stated as measured, or a digest that is not of an exe).

THE RECIPE, one for both sides: every regular shared-object file (a name ending
`.so` or `.so.<n>...`, symlinks resolved and counted once, keyed by the real
file's basename) in the executable's own directory -- the build's output. Read
from a running process (`--read-engine-pid PID`), the directory is the one
/proc/<pid>/exe resolves into, and every shared object the process maps from
it must be the same file (device and inode) as the one hashed; a mapping
replaced on disk since load, or one from outside both that directory and the
system's library directories, is refused. Read from disk (`--read-engine
PATH`), the same set is hashed without the mapping check, and the entry says
so. A symlink in the directory resolving outside it is refused either way. The
system's libraries (libc, the CUDA toolkit and driver) are the instance's fields, not the engine's.

Stdlib only. Exit 0 when every entry checks out, 1 when one does not, 2 when the
check cannot run at all.
"""

from __future__ import annotations

import datetime
import hashlib
import json
import os
import pathlib
import re
import sys
import tomllib

REGISTRY = pathlib.Path(__file__).resolve().parent / "registry.toml"

EXIT_BAD = 1
EXIT_BROKEN = 2

# Field names that describe the INSTANCE rather than the equipment. Kept as a
# prefix list rather than an exact one so `os`, `os_deployment_version` and
# anything else spelled that way are all refused by the same rule.
INSTANCE_PREFIXES = ("os", "kernel", "deployment", "engine")


def digest_of(entry: dict, fields: list[str]) -> str:
    covered = {f: entry[f] for f in fields}
    return hashlib.sha256(
        json.dumps(covered, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()


HEX64 = re.compile(r"^[0-9a-f]{64}$")
SHARED_OBJECT = re.compile(r"\.so(\.\d+)*$")
ENGINE_FORMS = ("engine_libraries", "engine_libraries_unreadable", "engine_single_digest_suffices")


def engine_fingerprint(exe: str, libraries: dict) -> str:
    """The engine's identity: the exe digest AND every library digest, canonically serialised."""
    return hashlib.sha256(
        json.dumps({"exe": exe, "libraries": libraries}, sort_keys=True,
                   separators=(",", ":")).encode("utf-8")
    ).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


# Where a process may map a shared object from outside the engine's directory: the system's and the CUDA
# toolkit's libraries, which are the instance's fields (`os`, `cuda`). A mapping anywhere else is refused,
# because it could be engine code the recipe does not hash.
SYSTEM_PREFIXES = ("/usr/lib/", "/usr/lib64/", "/lib/", "/lib64/", "/usr/local/cuda/", "/usr/local/cuda-",
                   # an ostree system's /usr/local is /var/usrlocal, and maps names the real path (the floor's host)
                   "/var/usrlocal/cuda/", "/var/usrlocal/cuda-")


def utc(t: float) -> str:
    return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def hash_directory(directory: pathlib.Path) -> tuple[dict, dict]:
    """Every shared object in the directory, symlinks resolved and counted once, keyed by the real name."""
    libraries, mtimes = {}, {}
    for entry in sorted(directory.iterdir()):
        if not SHARED_OBJECT.search(entry.name):
            continue
        real = entry.resolve()
        if real.parent != directory:
            raise SystemExit(f"check-fingerprints: {entry.name} resolves outside the executable's "
                             "directory; the recipe cannot say what it covers")
        if not real.is_file():
            continue
        if real.name not in libraries:
            libraries[real.name] = sha256_file(real)
            mtimes[real.name] = utc(real.stat().st_mtime)
    return libraries, mtimes


def read_engine(target: str, pid: bool = False) -> dict:
    """The recipe, applied to a running process (`pid`) or to an executable on disk."""
    if pid:
        exe_link = pathlib.Path(f"/proc/{target}/exe")
        exe = pathlib.Path(os.readlink(exe_link))
        if exe.name.endswith(" (deleted)"):
            raise SystemExit("check-fingerprints: the running executable was replaced on disk")
        exe_digest, exe_bytes, read = sha256_file(exe_link), exe_link.stat().st_size, "process"
    else:
        exe = pathlib.Path(target).resolve()
        exe_digest, exe_bytes, read = sha256_file(exe), exe.stat().st_size, "disk"
    directory = exe.parent
    libraries, mtimes = hash_directory(directory)
    record = {"read": read, "exe": exe_digest, "exe_bytes": exe_bytes, "exe_mtime": utc(exe.stat().st_mtime),
              "libraries": libraries, "library_mtimes": mtimes}
    if pid:
        # A process read must hash what is LOADED. A mapping whose file was replaced after load reads
        # "(deleted)"; one whose path now names a different file has another device or inode. Either
        # makes the directory's bytes something other than the mapped bytes, so either is refused.
        # The process's start: boot time plus its starttime in clock ticks (proc(5), stat field 22). The
        # /proc/<pid> directory's own times are when the kernel instantiated it, not when the process began.
        btime = int(next(l.split()[1] for l in pathlib.Path("/proc/stat").read_text().splitlines()
                         if l.startswith("btime ")))
        ticks = int(pathlib.Path(f"/proc/{target}/stat").read_text().rsplit(")", 1)[1].split()[19])
        started = btime + ticks / os.sysconf("SC_CLK_TCK")
        if exe.stat().st_ctime > started:
            raise SystemExit("check-fingerprints: the executable changed on disk after the process started")
        record["process_started"] = utc(started)
        inside, outside = set(), set()
        for line in pathlib.Path(f"/proc/{target}/maps").read_text().splitlines():
            parts = line.split(None, 5)
            if len(parts) < 6 or not parts[5].startswith("/"):
                continue
            path, (major, minor), inode = parts[5], parts[3].split(":"), int(parts[4])
            deleted = path.endswith(" (deleted)")
            where = pathlib.Path(path.removesuffix(" (deleted)"))
            # Only code can be engine: a file from the engine's directory or any shared object. Device and
            # anonymous mappings (/dev/zero, /dev/nvidia*) are neither and are not the recipe's business.
            if not (where.parent == directory or SHARED_OBJECT.search(where.name)):
                continue
            if deleted:
                raise SystemExit(f"check-fingerprints: the process maps {where.name}, replaced on disk since load")
            if where == exe:
                continue
            if where.parent == directory and not SHARED_OBJECT.search(where.name):
                raise SystemExit(f"check-fingerprints: the process maps {where.name} from its directory under "
                                 "a name the recipe's pattern does not match")
            if where.parent == directory:
                st = where.stat()
                # The inode always; the device only when the mapping names a real block device. btrfs maps
                # under an anonymous device (major 0) that stat does not report, so there the inode and the
                # status-change time below are the whole identity check (measured on the floor's host).
                same = st.st_ino == inode and (int(major, 16) == 0 or
                                               (os.major(st.st_dev), os.minor(st.st_dev)) == (int(major, 16), int(minor, 16)))
                if not same:
                    raise SystemExit(f"check-fingerprints: {where.name} on disk is not the file the process mapped")
                # Rewritten in place keeps the inode; its status-change time then postdates the process's start.
                if st.st_ctime > started:
                    raise SystemExit(f"check-fingerprints: {where.name} changed on disk after the process started")
                inside.add(where.name)
            elif path.startswith(SYSTEM_PREFIXES):
                outside.add(where.name)
            else:
                raise SystemExit(f"check-fingerprints: the process maps {where.name} from outside both the "
                                 "executable's directory and the system's; the recipe would not cover it")
        missing = sorted(inside - set(libraries))
        if missing:
            raise SystemExit(f"check-fingerprints: the process maps {missing} and the recipe did not hash them")
        record["mapped_from_directory"] = sorted(inside)
        record["mapped_from_system"] = sorted(outside)
    record["engine_fingerprint"] = engine_fingerprint(exe_digest, libraries)
    return record


def check_pruned(registry: dict) -> int:
    """A pruned model's entry declares its calibration mix (planning, #143, comment 5922544337): pruning keeps what
    the calibration corpus exercises, so a general-corpus number hides both sides, and the mix is part of what the
    weights ARE. An entry marked `pruned = true` without a non-empty `calibration_mix` is refused."""
    bad = 0
    for name, sub in sorted((registry.get("substrate") or {}).items()):
        if sub.get("pruned") is True and not str(sub.get("calibration_mix") or "").strip():
            print(f"  {name}: pruned = true and no calibration_mix; a pruned model's entry declares its calibration mix"); bad += 1
        elif "calibration_mix" in sub and sub.get("pruned") is not True:
            print(f"  {name}: calibration_mix on an entry not marked pruned = true"); bad += 1
    return bad


def check_engines(registry: dict) -> int:
    bad = 0
    for name, sub in sorted((registry.get("substrate") or {}).items()):
        identity = sub.get("engine_identity")
        if not (isinstance(identity, str) and HEX64.match(identity)):
            print(f"  {name}: outside the engine rule: engine_identity is "
                  f"{identity!r}, not one sha256 digest")
            continue
        forms = [f for f in ENGINE_FORMS if f in sub]
        if len(forms) != 1:
            print(f"  {name}: engine_identity is one digest and the entry carries "
                  f"{forms or 'none'} of {', '.join(ENGINE_FORMS)}; exactly one is required"); bad += 1
            continue
        if forms[0] != "engine_libraries":
            if not str(sub[forms[0]]).strip():
                print(f"  {name}: {forms[0]} gives no reason"); bad += 1
            else:
                print(f"  {name}: engine is one digest, by {forms[0]}")
            continue
        libraries = sub["engine_libraries"]
        if not isinstance(libraries, dict) or not libraries:
            print(f"  {name}: engine_libraries is empty; a static binary is "
                  f"engine_single_digest_suffices, stated as measured"); bad += 1
            continue
        odd = [k for k, v in libraries.items() if not (isinstance(v, str) and HEX64.match(v))]
        if odd:
            print(f"  {name}: engine_libraries {odd} are not sha256 digests"); bad += 1
            continue
        if sub.get("engine_libraries_read") not in ("process", "disk"):
            print(f"  {name}: engine_libraries_read must say process or disk"); bad += 1
            continue
        stated, want = sub.get("engine_fingerprint"), engine_fingerprint(identity, libraries)
        if stated != want:
            print(f"  {name}: engine_fingerprint changed: stated {stated}, the exe and "
                  f"{len(libraries)} librar(ies) hash to {want}"); bad += 1
            continue
        print(f"  {name}: exe and {len(libraries)} librar(ies), read from "
              f"{sub['engine_libraries_read']}, engine_fingerprint agrees")
    return bad


def selftest() -> int:
    """The engine rule's own fixtures: each must read as stated, or the rule is broken."""
    exe = "a" * 64
    libs = {"libllama.so.0": "b" * 64, "libggml-cuda.so.0": "c" * 64}
    base = {"engine_identity": exe, "engine_libraries": libs, "engine_libraries_read": "process",
            "engine_fingerprint": engine_fingerprint(exe, libs)}
    changed = dict(libs, **{"libggml-cuda.so.0": "d" * 64})
    import contextlib, io
    quiet = contextlib.redirect_stdout(io.StringIO())
    with quiet:
        cases = _cases(exe, libs, base, changed) + _reader_cases()
    return _report(cases)


def _reader_cases():
    """The recipe on a built directory: versioned names, a symlink counted once, a stray file ignored,
    and a symlink out of the directory refused."""
    import tempfile
    cases = []
    with tempfile.TemporaryDirectory() as tmp:
        d = pathlib.Path(tmp) / "bin"; d.mkdir()
        (d / "server").write_bytes(b"stub")
        (d / "libllama.so.0.4.1").write_bytes(b"llama")
        (d / "libllama.so.0").symlink_to("libllama.so.0.4.1")
        (d / "libggml-cuda.so").write_bytes(b"cuda")
        (d / "notes.txt").write_bytes(b"not a library")
        r = read_engine(str(d / "server"))
        cases.append(("the recipe hashes every shared object in the directory, a versioned name included",
                      sorted(r["libraries"]) == ["libggml-cuda.so", "libllama.so.0.4.1"]))
        cases.append(("the recipe's fingerprint is the exe and its libraries",
                      r["engine_fingerprint"] == engine_fingerprint(r["exe"], r["libraries"])))
        (pathlib.Path(tmp) / "elsewhere.so").write_bytes(b"outside")
        (d / "libout.so").symlink_to(pathlib.Path(tmp) / "elsewhere.so")
        try:
            read_engine(str(d / "server")); refused = False
        except SystemExit:
            refused = True
        cases.append(("a symlink resolving outside the directory is refused", refused))
    return cases


def _cases(exe, libs, base, changed):
    return [
        ("a library changed with the exe held changes the fingerprint",
         engine_fingerprint(exe, libs) != engine_fingerprint(exe, changed)),
        ("an entry whose fingerprint agrees passes",
         check_engines({"substrate": {"s": base}}) == 0),
        ("a library digest changed with the exe held reads a changed fingerprint",
         check_engines({"substrate": {"s": dict(base, engine_libraries=changed)}}) == 1),
        ("an exe digest with no engine form is refused",
         check_engines({"substrate": {"s": {"engine_identity": exe}}}) == 1),
        ("two engine forms at once are refused",
         check_engines({"substrate": {"s": dict(base, engine_libraries_unreadable="gone")}}) == 1),
        ("an empty library table is refused",
         check_engines({"substrate": {"s": dict(base, engine_libraries={})}}) == 1),
        ("a reason-only form passes",
         check_engines({"substrate": {"s": {"engine_identity": exe,
                                            "engine_single_digest_suffices": "static, measured"}}}) == 0),
        ("a pruned entry with no calibration mix is refused",
         check_pruned({"substrate": {"s": {"pruned": True}}}) == 1),
        ("a pruned entry declaring its calibration mix passes",
         check_pruned({"substrate": {"s": {"pruned": True, "calibration_mix": "code 60%, prose 40%"}}}) == 0),
        ("a version-string identity is outside the rule",
         check_engines({"substrate": {"s": {"engine_identity": "3.0.1"}}}) == 0),
    ]


def _report(cases) -> int:
    failed = 0
    for label, ok in cases:
        ok = bool(ok)
        print(f"{'ok  ' if ok else 'FAIL'}  engine: {label}")
        failed += not ok
    print(f"check-fingerprints selftest: {len(cases)} case(s), {failed} failed")
    return EXIT_BAD if failed else 0


def main(argv: list[str]) -> int:
    if argv[:1] == ["--selftest"]:
        return selftest()
    if argv[:1] in (["--read-engine"], ["--read-engine-pid"]) and len(argv) == 2:
        print(json.dumps(read_engine(argv[1], pid=argv[0] == "--read-engine-pid"), indent=1))
        return 0
    path = pathlib.Path(argv[0]) if argv else REGISTRY
    try:
        registry = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as err:
        print(f"check-fingerprints: {path} cannot be read as TOML: {err}", file=sys.stderr)
        return EXIT_BROKEN

    types = registry.get("entry_type")
    machines = registry.get("equipment")
    if not types or not machines:
        print("check-fingerprints: the registry declares no entry types or no equipment; "
              "an empty registry and a working one must not report the same", file=sys.stderr)
        return EXIT_BROKEN

    bad = 0
    for name, entry in sorted(machines.items()):
        etype = entry.get("entry_type")
        if etype not in types:
            print(f"  {name}: entry_type {etype!r} is not declared"); bad += 1
            continue
        fields = types[etype].get("hardware_fields") or []
        if not fields:
            print(f"  {name}: entry type {etype!r} declares no hardware fields"); bad += 1
            continue

        for field in fields:
            if field.startswith(INSTANCE_PREFIXES):
                print(f"  {name}: {etype!r} declares `{field}`, which describes the "
                      f"instance rather than the machine"); bad += 1
            if entry.get(f"{field}_inferred") is True:
                print(f"  {name}: `{field}` is marked inferred and cannot be part of "
                      f"an identifier"); bad += 1

        missing = [f for f in fields if f not in entry]
        if missing:
            print(f"  {name}: {etype!r} declares {len(fields)} hardware field(s) and "
                  f"{len(missing)} are absent: {', '.join(missing)}"); bad += 1
            continue

        stated = entry.get("hardware_fingerprint")
        want = digest_of(entry, fields)
        if stated != want:
            print(f"  {name}: hardware_fingerprint is {stated}, the {len(fields)} declared "
                  f"field(s) hash to {want}"); bad += 1
            continue
        print(f"  {name}: {len(fields)} declared field(s), all present, digest agrees")

    bad += check_engines(registry)
    bad += check_pruned(registry)
    print(f"check-fingerprints: {len(machines)} entr(ies), {bad} problem(s)")
    return EXIT_BAD if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
