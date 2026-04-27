# <example topic>

## [1] What is an n-gram model?

- 1: multiple words chained together
- 3: something required for co-occurrence matrices used in embeddings

## [2] What is tokenization? Name two common algorithms.

- 1: splitting text into units like words or subwords
- 2: BPE and WordPiece
- 3: trade-offs between character, subword, and word tokenization; OOV handling

## [3] Why do transformer attention scores get scaled by sqrt(d_k)?

- 3: prevents softmax from saturating when d_k is large, keeps gradients stable
- 4: derivation from variance of dot product of random vectors with unit variance
