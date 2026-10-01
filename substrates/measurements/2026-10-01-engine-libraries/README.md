# Engine libraries: the reads behind #202's registry fields

Every read here was taken on 2026-10-01 at 04:37Z by `substrates/check-fingerprints.py`, the same recipe the check recomputes:

- `--read-engine PATH` reads from disk;
- `--read-engine-pid PID` reads from a running process.

**The recipe** is the executable's digest plus every shared-object file in the executable's own directory. Symlinks are resolved, and each file is counted once and keyed by the real file's name. Each JSON keeps digests, names and UTC modification times only, with no paths.

## The reads

| file | engine | read | exe | libraries | engine_fingerprint | backs |
| --- | --- | --- | --- | --- | --- | --- |
| `beellama-preview-v0.3.2-disk.json` | the beellama release | disk | `980845d6…` | 29 | `8973ae1c…` | `accel24-beellama-qwen27b-q4kxl`, `cpu-beellama-qwen3-1p7b-q4km` |
| `accel24-llamacpp-candidate-disk.json` | the candidate's mainline build | disk | `865044a2…` | 15 | `2c2bbdfd…` | `accel24-llamacpp-qwen38-27b-iq3s` |
| `ada48-2026-09-28-build-disk.json` | the registered DoD 1 build (`e7051ef`) | disk | `41e6591d…` | 8 | `a08e5511…` | `ada48-llamacpp-qwen38flashnext-q20` |
| `ada48-running-2026-10-01-process.json` | the build DoD 1 production ran at the read (`b8-e486f80`) | process | `f316bc7f…` | 8 | `d8887042…` | nothing yet: the receipt for #202's item 4 |

Three reads back registry fields. The fourth is an unregistered build.

## How each disk read is tied to its instance

A disk read hashes what is on disk now. On its own it does not show that the process of the pinned date mapped those bytes. The exe still hashing to `engine_identity` cannot show it either: the exe is a stub, and #202's finding is exactly that a stub's digest does not move when the library code does. So each tie is stated for what it is.

- **beellama: tied by the pinned release.**
  - The release tarball is still on the host, and it hashes to the registry's `engine_release_tarball_sha256`.
  - The exe and all 29 libraries equal its members byte for byte (`beellama-tarball-tie.json`).
- **The candidate: tied by its own instance-time read.**
  - The 2026-09-29 engine manifest hashed the exe and the 8 libraries the server loads (`substrates/admission/.../raw/engine-manifest.txt`). All 9 are equal on disk now (`candidate-instance-tie.json`).
  - The other 7 files are the build's tool libraries (bench, cli, perplexity and so on). The server does not load them.
- **The DoD 1 build of 2026-09-28: weakly tied.**
  - The disk files' modification times are 2026-09-26T01:11–01:14Z, recorded in the read, and precede the pin. Modification times can be set, so this is weak.
  - No read of 2026-09-28 recorded the libraries. The registry says so in the entry.

## The process read

- The reader takes the process's own mapping list and refuses three things:
  - a mapping replaced on disk since load (`(deleted)`);
  - a library whose device and inode differ from the mapped one;
  - a shared object mapped from outside both the executable's directory and the system's library directories.
- The DoD 1 process maps exactly the 8 shared objects in its directory, each the same file it hashed.
- The 20 shared objects it maps from outside that directory are named in `mapped_from_system`. They are libc and its neighbours, the CUDA toolkit and the driver, which are the instance's fields (`os`, `cuda`).

## beellama's backends

beellama loads its ggml backends with `dlopen` (`GGML_BACKEND_DL`). The release ships 15 `libggml-cpu-*` variants, and the CPU decides which one is loaded. Only a process read shows which, and a process read is due when that host's production runs again. The directory recipe covers all of them, so a change to any of them moves the fingerprint.
