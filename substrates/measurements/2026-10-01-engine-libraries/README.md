# Engine libraries: the four reads behind #202's registry fields

Each read below was taken on 2026-10-01 by `substrates/check-fingerprints.py --read-engine`, the same recipe the check recomputes. The recipe covers the executable's digest plus every shared-object file in the executable's own directory (symlinks resolved, keyed by the real file's name). Each file keeps digests and names only, with no paths.

## The reads

| file | engine | read | exe | libraries | engine_fingerprint |
| --- | --- | --- | --- | --- | --- |
| `ada48-2026-09-28-build-disk.json` | `ada48-llamacpp-qwen38flashnext-q20`, the registered build (`e7051ef`) | disk | `41e6591d…` | 8 | `a08e5511…` |
| `ada48-running-2026-10-01-process.json` | the build production ran at the read (`b8-e486f80`), **not registered** | process | `f316bc7f…` | 8 | `d8887042…` |
| `beellama-preview-v0.3.2-disk.json` | `accel24-beellama-qwen27b-q4kxl` and `cpu-beellama-qwen3-1p7b-q4km` | disk | `980845d6…` | 29 | `8973ae1c…` |
| `accel24-llamacpp-candidate-disk.json` | `accel24-llamacpp-qwen38-27b-iq3s` | disk | `865044a2…` | 15 | `2c2bbdfd…` |

## What "disk" means here

- **Disk reads were the only option.** Three of the four engines were not running when they were read:
  - the 24 GiB host's production was down;
  - the candidate serves only in windows;
  - the Ada host now runs a newer build.
- **Each disk read is tied to its instance two ways.**
  - The executable in the read directory still hashes to the registry's `engine_identity`.
  - The directory's files are dated before the instance they back: 2026-06-14, 2026-09-22 and 2026-09-25 respectively.
- **They are not proven to be the bytes that were mapped.** A library replaced in place after the instance was pinned would not show. The registry marks every such field `engine_libraries_read = "disk"` for that reason.

## Two findings from the reads

- **The stub.** Every llama.cpp executable read here is a stub of 17,872–17,960 bytes. The engine itself lives in its libraries.
- **beellama loads its backends at run time.** It uses `dlopen` (`GGML_BACKEND_DL`), so `ldd` names 6 of its 29 libraries. Which `libggml-cpu-*` variant gets loaded depends on the CPU, and only a process read shows it. That is why the recipe hashes the whole directory rather than what `ldd` lists.

## The process read

- On the Ada host the process maps exactly the 8 shared objects in its directory. Its other 20 mappings are system, CUDA toolkit and driver libraries, which are instance fields (`os`, `cuda`).
- That read is the receipt for #202's item 4. Its new instance row waits on the maintainer's word on the serving line.
