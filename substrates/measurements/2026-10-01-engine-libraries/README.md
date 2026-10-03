# Engine libraries: the reads behind #202's registry fields

The five reads here were taken on 2026-10-01, between 04:52Z and 05:12Z, by `substrates/check-fingerprints.py`, the same recipe the check recomputes:

- `--read-engine PATH` reads from disk;
- `--read-engine-pid PID` reads from a running process.

**The recipe** is the executable's digest plus every shared-object file in the executable's own directory. Symlinks are resolved, and each file is counted once and keyed by the real file's name. Each JSON keeps digests, names and UTC modification times only, with no paths.

## The reads

| file | engine | read | exe | libraries | engine_fingerprint | backs |
| --- | --- | --- | --- | --- | --- | --- |
| `beellama-floor-process.json` | the beellama release, as the restored floor's production maps it | process | `980845d6…` | 29 | `8973ae1c…` | `accel24-beellama-qwen27b-q4kxl` |
| `beellama-preview-v0.3.2-disk.json` | the beellama release | disk | `980845d6…` | 29 | `8973ae1c…` | `cpu-beellama-qwen3-1p7b-q4km` |
| `accel24-llamacpp-candidate-disk.json` | the candidate's mainline build | disk | `865044a2…` | 15 | `2c2bbdfd…` | `accel24-llamacpp-qwen38-27b-iq3s` |
| `ada48-2026-09-28-build-disk.json` | the DoD 1 build of 2026-09-28 (`e7051ef`) | disk | `41e6591d…` | 8 | `a08e5511…` | `ada48-llamacpp-qwen38flashnext-q20`'s 2026-09-28 instance |
| `ada48-running-2026-10-01-process.json` | the DoD 1 build of 2026-10-01 (`b8-e486f80`), as it ran at 04:52Z | process | `f316bc7f…` | 8 | `d8887042…` | `ada48-llamacpp-qwen38flashnext-q20`'s engine fields since its 2026-10-01 instance |

All five reads back registry fields. The DoD 1 process read, taken read-only at 04:52Z, was at first a receipt only, held while the maintainer's line was unsettled. It now backs `ada48`'s engine fields, through the instance of 2026-10-01:
- The inference seat confirmed that the host has served the registered Q2_0 line on `b8-e486f80` since 2026-09-30 at about 23:20Z. The other quant never served production there.
- Its own process read, at about 16:10Z, equals this one field for field (`substrates/measurements/2026-10-01-ada48-e486f80/`).

## How each disk read is tied to its instance

A disk read hashes what is on disk now. On its own it does not show that the process of the pinned date mapped those bytes. The exe still hashing to `engine_identity` cannot show it either: the exe is a stub, and #202's finding is exactly that a stub's digest does not move when the library code does. So each tie is stated for what it is.

- **beellama: tied by the pinned release, and read from its process.** The floor's process read, taken minutes after its restore, gives the same 29 files and fingerprint as the disk read.
  - The release tarball is still on the host, and it hashes to the registry's `engine_release_tarball_sha256`.
  - Its regular members' digests were read on the host at 2026-10-01T04:52Z into `beellama-tarball-members.json`. That read was hand-run: Python's `tarfile` over the tarball, with a sha256 of every regular member. The exe and all 29 libraries equal them.
- **The candidate: tied by its own instance-time read.**
  - The 2026-09-29 engine manifest hashed the exe and the 8 libraries the server loads (`substrates/admission/accel24-llamacpp-qwen38-27b-iq3s/9d84a552fc94/raw/engine-manifest.txt`). All 9 are equal on disk now.
  - The other 7 files are the build's tool libraries (bench, cli, perplexity and so on). The server does not load them.
- **The DoD 1 build of 2026-09-28: weakly tied.**
  - The disk files' modification times are 2026-09-26T01:11–01:14Z, recorded in the read, and precede the pin. Modification times can be set, so this is weak.
  - No read of 2026-09-28 recorded the libraries. The registry says so in the entry.

Both ties recompute from committed files with `python3 tie.py`. It also checks that each registry table equals the read it cites and that no input is empty, and exits 1 on any failure. No gate runs it; it is a recompute for a reader.

## The process read

- The reader takes the process's own mapping list and refuses five things:
  - a mapping replaced on disk since load (`(deleted)`);
  - a library whose device and inode differ from the mapped one, or whose status-change time is later than the process's start (a file rewritten in place);
  - a file mapped from the directory under a name the recipe's pattern misses;
  - a shared object mapped from outside both the executable's directory and the system's library directories;
  - an executable changed on disk after the process started.
- **The floor's process** maps 9 of its directory's 29 files, and only those 9 get the inode and status-change checks; the other 20 are hashed from disk as a disk read would hash them: the core libraries, `libggml-cuda.so`, `libggml-rpc.so` and `libggml-cpu-haswell.so`, this CPU's variant. Each is the same inode it hashed, and none changed after the process started.
- **The 18 shared objects it maps from outside the directory** are named in `mapped_from_system`: libc and its neighbours, the CUDA toolkit and the driver. These are the instance's fields.
- **Two reader fixes came from this host:**
  - Its `/usr/local` is `/var/usrlocal` (ostree), and the maps name the real path, so that prefix is a system prefix.
  - Its btrfs maps files under an anonymous device (major 0) that `stat` does not report. There, the device number is not compared; the inode and the status-change time are.

## beellama's backends

beellama loads its ggml backends with `dlopen` (`GGML_BACKEND_DL`). The release ships 15 `libggml-cpu-*` variants, and the CPU decides which one is loaded. The floor's process read shows `libggml-cpu-haswell.so`. The directory recipe covers all of them, so a change to any of them moves the fingerprint.

## Limits of a process read

- On btrfs and any filesystem that maps under an anonymous device, the identity check is the inode plus the status-change time, without the device. `/proc/<pid>/map_files`, which would give the mapped file's own identity, needs privilege this reader does not have.

- The system prefixes are trusted as system. A build installed under `/usr/lib` would be read as system libraries, giving an empty library table. The check refuses an empty table, but the reader does not.
- The reader runs in the host's mount namespace. A server inside a container whose engine directory is not visible at the same path would fail the read rather than refuse it cleanly.
