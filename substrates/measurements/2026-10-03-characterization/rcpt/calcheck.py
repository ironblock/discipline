# calcheck.py: does any 200-character window of the KLD corpora occur verbatim in ExLlamaV3's bundled calibration
# files? Windows start every 1,000 characters of each corpus file (as read, no normalization); each window is
# searched in every calibration file's full text. Prints one line per corpus and per calibration file.
import os, sys, exllamav3
cal = os.path.join(os.path.dirname(exllamav3.__file__), "conversion", "standard_cal_data")
cal_files = sorted(f for f in os.listdir(cal) if f.endswith(".utf8"))
texts = {f: open(os.path.join(cal, f), encoding="utf-8", errors="replace").read() for f in cal_files}
for f in cal_files:
    print(f"calibration {f} chars={len(texts[f])}")
for corpus in sys.argv[1:]:
    t = open(corpus, encoding="utf-8", errors="replace").read()
    wins = [t[i:i + 200] for i in range(0, len(t) - 200, 1000)]
    hits = sum(1 for w in wins if any(w in x for x in texts.values()))
    print(f"corpus {os.path.basename(corpus)} chars={len(t)} windows={len(wins)} found_in_calibration={hits}")
