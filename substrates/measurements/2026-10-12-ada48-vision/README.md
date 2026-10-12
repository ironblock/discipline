# Flash-Next on rtx6000ada-host: vision on, the tower's weights in system RAM (2026-10-12)

The maintainer asked for vision on the 48 GiB host's Flash-Next line, with the projector off the GPU. This is the change, what it costs, and the proof that the served model sees an image. It took two restarts, which made instances `2026-10-12a` and `2026-10-12b` of `ada48-tabbyapi-exl3-qwen38flashnext-2p05`. 2026-10-12b is what serves.

## What `vision_offload` does

It does not run the tower on the CPU. Read from ExLlamaV3 1.5.4 (16a4979):
- **Weights move to host RAM, and the GPU reads them there.** TabbyAPI's `vision_offload` sets `infer_params.vision_pinned`. After load, `Linear.pin_linears()` (`modules/linear.py`) moves each vision linear layer's bulk weights into pinned host memory and swaps in a zero-copy CUDA alias (`ext.pinned_cuda_view`). The kernels then read those weights over PCIe in place.
- **Compute stays on the GPU.** Small tensors (bias, suh/svh) and every activation stay in VRAM.
- **No path runs the tower on the CPU.** The only `prefer_cpu` module in 1.5.4 is DeepSeek-V4's MTP embedding, and CPU MoE offload covers text experts only.

**How it is confirmed on the host.** `/v1/model` does not report `vision_offload`, so the evidence is:
- the live config's `vision_offload: true` (`configs/config-redacted.yml`);
- the startup log's `Keeping vision model weights in system RAM (vision_offload).`, followed by `Loading vision modules 30/30`.

## The tower

`config.json` carries a `vision_config`:

| | |
|---|---|
| layers | 27 |
| hidden size | 1152 |
| heads | 16 |
| patch | 16 |
| spatial merge | 2 |
| output size | 2560 |

`quantization_config.json` sets `vision_bits` to 4. The tower is 987 tensors and 428.7 MiB, all in `model-00005-of-00005.safetensors`. The weights were already in the registered digests; nothing new was downloaded.

## The change, as served (instance 2026-10-12b)

| | 2026-10-04b (before) | 2026-10-12a (19:10:57–19:36:08Z) | **2026-10-12b (serving)** |
|---|---|---|---|
| `model.vision` / `vision_offload` | off / off | true / true | **true / true** |
| `model.cache_size` | 606,208 | 573,440 | **524,288** (= `max_seq_len`) |
| overlay `preprocessor_config.json` | link to native | real file, `longest_edge` 4,194,304 (a cap) | **link to native** (`longest_edge` 16,777,216, the model's own) |
| live config sha256 | b5db18d5… (4192f0b8… after the 10-05 sampler preset) | a9684870… | **a41e8ab7…** |

- **Why 2026-10-12a existed.** It was the seat's proposal: a smaller pool cut to make room for the tower's activations, plus an image cap. The maintainer had asked for 524,288 and no cap, so 2026-10-12b corrected it 25 minutes later. Its config and capped preprocessor are kept in `configs/instance-2026-10-12a/`.
- **Files:**
  - `configs/config-redacted.yml` is 2026-10-12b's live config with three non-serving values redacted (`configs/config-redaction.diff`, which covers both instances).
  - `configs/qwen38_thinking.yml` is the sampler preset, unchanged since 2026-10-05 (sha256 8da14115…).
  - `vtest.py` is the test script. It reads the key from the server's token file, never from an argument.

## Results (`results.txt`)

- **Four confirmations on 2026-10-12b:**
  - up: health 200 at 19:36:36Z, 28 s after the stop;
  - right model: `/v1/model` reports `Qwen3.8-Flash-Next-exl3-2.05bpw-yarn2`, max_seq_len 524288, cache_size 524288, cache_mode 8,8;
  - warmed: a short completion answered "ready";
  - required settings: the startup log shows vision offload, the 30 vision modules, the sampler preset and ExLlamaV3 1.5.4.
- **The image was seen,** in every request. The synthetic image (`vtest.py`'s `img()`) holds the text VIOLET 4729, a red circle and a blue square. Every reply named all three:

  | instance | request | prompt tokens | time | peak VRAM | free at peak |
  |---|---|---|---|---|---|
  | 12b | **4096² (16.8 MP, the model's limit), alone** | 16,461 | 13.3 s | 47,942 MiB | 1,198 MiB |
  | 12b | 1024², alone | 1,101 | 1.1 s | 47,946 MiB | 1,194 MiB |
  | 12b | **4096², in a full fill** (4 × ~125k text tokens + the image: 515,996 of 524,288) | 16,461 | 112.2 s | 47,976 MiB | **1,164 MiB** |
  | 12a | 1024², alone | 1,101 | 1.6 s | 47,422 MiB | 1,718 MiB |
  | 12a | 2048², alone | 4,173 | 2.9 s | 47,646 MiB | 1,494 MiB |
  | 12a | 4096², resized to the cap | 4,173 | 2.8 s | 47,648 MiB | 1,492 MiB |
  | 12a | 2048², beside a 94,654-token text request | 4,173 | 20.6 s | 47,650 MiB | 1,490 MiB |
  | 12a | 2048², in a full fill (4 × ~137.8k) | 4,173 | 131.9 s | 47,664 MiB | 1,476 MiB |

  A full-size image inside a full fill leaves 1,164 MiB free, which clears the 300 MiB bar (#143 5984376546). The 2026-10-04 fill at 606,208 without vision left 978 MiB.
- **VRAM after load:** 46,100 MiB on 12b and 47,000 on 12a, against 47,598 at 606,208 without vision. A large image's activations stay reserved by torch's allocator afterward: about 47.9 GiB is resident after the first 16.8 MP image.

## Engine identity

The config is one of the ten components, so the composite changes. `identity.json` is `substrates/admission/depth-probe/tabby_identity.py`'s output for 2026-10-12b, run on the host with the serving venv. It reports engine_identity `158b4cd77f188b5f51e5e8a4af8047d886260e767765f85df68eab6d9da8a6ea`. Every other component equals instance 2026-10-04b's. 2026-10-12a's config digest (c4d7ddeb…) is recorded on its instance row.

## Not measured

- Image-understanding quality beyond this planted-token check (no benchmark).
- Speed on real photographs.
- Several large images in one request.
- Whether `shortest_edge` needs raising as the floor's did (img1024). The 1024² synthetic image at the default read correctly.
