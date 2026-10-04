# tokcheck.py: tokenizer identity of a GGUF, from its metadata only: pre-tokenizer, vocabulary size, and the sha256
# of the token list (tokens joined by NUL), the merges and the token types.
import hashlib, sys, numpy as np
from gguf import GGUFReader
for path in sys.argv[1:]:
    r = GGUFReader(path)
    def field(k):
        f = r.fields.get(k)
        return f
    def strs(k):
        f = field(k); return [bytes(f.parts[i]).decode("utf-8", "replace") for i in f.data]
    def scal(k):
        f = field(k); return bytes(f.parts[f.data[0]]).decode() if f.types[0].name == "STRING" else f.parts[f.data[0]][0]
    toks = strs("tokenizer.ggml.tokens"); merges = strs("tokenizer.ggml.merges")
    tt = np.array([field("tokenizer.ggml.token_type").parts[i][0] for i in field("tokenizer.ggml.token_type").data])
    h = lambda xs: hashlib.sha256("\0".join(xs).encode()).hexdigest()[:16]
    print(f"{path.split('/')[-1]} pre={scal('tokenizer.ggml.pre')} model={scal('tokenizer.ggml.model')} n_vocab={len(toks)} "
          f"tokens_sha={h(toks)} merges={len(merges)} merges_sha={h(merges)} types_sha={hashlib.sha256(tt.tobytes()).hexdigest()[:16]} "
          f"bos={scal('tokenizer.ggml.bos_token_id') if 'tokenizer.ggml.bos_token_id' in r.fields else None} eos={scal('tokenizer.ggml.eos_token_id')}")
