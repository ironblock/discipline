+++
hypothesis = "State the claim being tested, in one sentence, so that it could be wrong."
result = "supported"
product_sha256 = "a4c8f7287a11a48f011777a9d8d7c9e446e08fbb2092c9ac7e7247057719f3a2"
controls_run = ["null-regimen"]
known_defects = []
turns = 2
prefill_tokens_total = 2048

[regime]
arm = "baseline"
substrate = "local"
dogma_version = 0
+++

# Fenced

~~~
## Observation
~~~ this trailing text means CommonMark does not close the fence here
## Hypothesis
## Test
## Results
## Conclusion
~~~
