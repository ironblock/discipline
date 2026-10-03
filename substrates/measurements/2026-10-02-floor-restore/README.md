# The floor's restore of 2026-10-02 (#143)

This is the evidence for instance `2026-10-02` of `accel24-beellama-qwen27b-q4kxl` in `substrates/registry.toml`.

The box slept after the 2026-10-01 window. It was rebooted 2026-10-02T23:13Z, into a new operating system image, 44.20260929. The provisioning script then restored production, and health first returned 200 at 23:16:02Z.

| file | what |
| --- | --- |
| `fingerprint-check.txt` | `fingerprint.py --check` against the 2026-10-01 capture: drift in `deployment_digest` and `kargs` only, and in kargs only the ostree deployment path |
| `substrate-2026-10-02-postupdate.json` | the new capture (`--hash-models`), home paths collapsed to `~` and the root filesystem UUID in kargs redacted (also in `fingerprint-check.txt`); the raw capture's digest is in the registry's `pinned_by` |
| `canary.txt` | canary e0827, k = 36: 35/36, PASS, with the tool's 95% interval |
| `canary-interval.txt` | the same rate's 99% Wilson interval, #114's level, as the registry cites it |
| `os.txt` | `rpm-ostree status` (booted, then rollback: version, base-checksum prefix, image timestamp), the booted deployment's own checksum, `uname -r`, the driver and the driver library the server maps |
| `process.txt` | the server's pid, the sha256 of its binary through `/proc/<pid>/exe`, its start time, and the count of listeners on its port |

verify-box passed before and after the restore. Netconsole was re-armed, and its round-trip check passed.
