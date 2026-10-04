# exl3kld.py: KL divergence of an EXL3 model against a llama-perplexity --kl-divergence-base file,
# reproducing llama.cpp's tools/perplexity kl_divergence() exactly (same tokens, same scored positions
# [n_ctx/2, n_ctx-1), same uint16 base log-prob decoding, same -16 cutoff, same top-1 rule).
# Usage: python exl3kld.py -m <exl3 dir> [model_init flags: -mcl, -ngr, ...] --base base-code.kld --tag x
import argparse, struct, sys, time, math
import numpy as np, torch
from exllamav3 import model_init


def main():
    ap = argparse.ArgumentParser(allow_abbrev=False)
    model_init.add_args(ap, cache=False)
    ap.add_argument("--base", action="append", required=True)
    ap.add_argument("--tag", default="exl3")
    ap.add_argument("--selfcheck", action="store_true", help="only parse the base and print its own PPL")
    args = ap.parse_args()

    bases = []
    for path in args.base:
        f = open(path, "rb")
        assert f.read(8) == b"_logits_"
        n_ctx, n_vocab, n_chunk = struct.unpack("<iii", f.read(12))
        tokens = np.frombuffer(f.read(4 * n_ctx * n_chunk), dtype=np.int32).reshape(n_chunk, n_ctx)
        bases.append((path, f, n_ctx, n_vocab, n_chunk, tokens))

    model = None
    if not args.selfcheck:
        t0 = time.time()
        model, config, _, tokenizer = model_init.init(args, override_dynamic_seq_len=2048,
                                                      max_output_size=2048, max_output_factor=5)
        print(f"[{args.tag}] loaded in {time.time()-t0:.0f}s; VRAM used "
              f"{(torch.cuda.mem_get_info()[1]-torch.cuda.mem_get_info()[0])/2**30:.1f} GiB", flush=True)

    dev = torch.device("cpu" if args.selfcheck else "cuda:0")
    BLK = 256
    for path, f, n_ctx, n_vocab, n_chunk, tokens in bases:
        first = n_ctx // 2
        n_rows = n_ctx - 1 - first
        nv = 2 * ((n_vocab + 1) // 2) + 4
        kl_all, nll_sum, nll_base_sum, same, cnt = [], 0.0, 0.0, 0, 0
        t0 = time.time()
        for c in range(n_chunk):
            raw = np.frombuffer(f.read(n_rows * nv * 2), dtype=np.uint16).reshape(n_rows, nv)
            tgt_all = torch.from_numpy(tokens[c, first + 1:n_ctx].astype(np.int64))
            lg_all = None
            if model is not None:
                ids = torch.from_numpy(tokens[c].astype(np.int64))[None, :]
                with torch.inference_mode():
                    logits = model.forward(ids, {"attn_mode": "flash_attn_nc"})
                lg_all = logits[0, first:n_ctx - 1, :n_vocab]
                del logits
            for r0 in range(0, n_rows, BLK):
                r1 = min(n_rows, r0 + BLK)
                blk = raw[r0:r1]
                hdr = blk[:, :4].copy().view(np.float32)            # per row: scale, min_log_prob
                scale = torch.from_numpy(hdr[:, 0].copy()).to(dev).unsqueeze(1)
                minlp = torch.from_numpy(hdr[:, 1].copy()).to(dev).unsqueeze(1)
                q = torch.from_numpy(blk[:, 4:4 + n_vocab].astype(np.int32)).to(dev)
                base_lp = scale * q.float() + minlp
                tgt = tgt_all[r0:r1].to(dev)
                nll_base_sum += float(-(base_lp.gather(1, tgt[:, None])).sum())
                if lg_all is not None:
                    lg = lg_all[r0:r1].to(dev).float()
                    lp = lg - torch.logsumexp(lg, dim=1, keepdim=True)
                    nll_sum += float(-(lp.gather(1, tgt[:, None])).sum())
                    kl = torch.where(base_lp > -16.0, base_lp.exp() * (base_lp - lp), torch.zeros_like(lp)).sum(1)
                    kl_all.append(kl.cpu())
                    same += int((lg.argmax(1) == q.argmax(1)).sum())  # both sides: first maximum, as llama.cpp
                    del lg, lp, kl
                del q, base_lp
                cnt += r1 - r0
            del lg_all
            if model is None:
                continue
            torch.cuda.empty_cache()
            print(f"[{args.tag}] {path.split('/')[-1]} chunk {c+1}/{n_chunk} running KLD "
                  f"{torch.cat(kl_all).mean():.5f} PPL {math.exp(nll_sum/cnt):.4f} ({time.time()-t0:.0f}s)", flush=True)
        ppl_base = math.exp(nll_base_sum / cnt)
        if model is None:
            print(f"[selfcheck] {path}: n_ctx={n_ctx} n_vocab={n_vocab} n_chunk={n_chunk} base PPL {ppl_base:.4f}")
            continue
        k = torch.cat(kl_all).double()
        p = same / cnt
        print(f"RESULT {args.tag} {path.split('/')[-1]}: PPL(Q) {math.exp(nll_sum/cnt):.6f} PPL(base) {ppl_base:.6f} "
              f"Mean KLD {k.mean():.6f} ± {k.std()/math.sqrt(len(k)):.6f} 99.9% KLD {torch.quantile(k.float(), 0.999):.6f} "
              f"Median KLD {k.median():.6f} Same top {100*p:.3f} ± {100*math.sqrt(p*(1-p)/cnt):.3f} % n={cnt}", flush=True)


if __name__ == "__main__":
    main()
